# VAL-288 — Restore a passing full verification gate

Status: complete; full gate/evaluator passed and signed ca 0.1.40 installed once (2026-10-22).

## Goal and acceptance

Repair the causes of the current full verification failures without excluding tests, weakening assertions, changing accepted snapshots wholesale, lowering production safety floors, or converting failed receipts to passes. The normal entry point is `./scripts/verify.sh`; acceptance requires its complete report and independent evaluation to pass on frozen combined source, followed by one recoverable signed installation and actual entry-point readback.

The installed baseline is signed ca 0.1.39. Source inventory and exact before-images are under `scratch/VAL-288/`; the current executed baseline is `scratch/AGY-295/gate-v2/`: default 4141 passed / 170 failed / 43 ignored, all-features 4296 / 171 / 44, two format hunks and 57 lint message/file records. Nine official native terminal checks already pass. Historical VAL-288 counts are stale and remain historical evidence.

## Repair checklist

- [x] Capture source/installed baseline and inspect current report, evaluation and raw failures. Assign narrow independent failure families.
- [x] Repair deterministic TUI test version geometry and chord timeout behavior while preserving current visible features and accepted output criteria. All 255 render checks pass; one pre-existing ignored case remains ignored.
- [x] Restore questionnaire/personality CLI integration and native verification evidence; narrow implicit Explore presets to permitted tools while leaving explicit grants strict and network denied. Eight questionnaire CLI and native verification regressions pass. The new policy unit and all three real cross-provider CLI integration cases pass.
- [x] Isolate learning-vault resource fixtures from host capacity using deterministic per-instance input while retaining production disk/health guards; preserve adversarial Git clean-filter coverage. All 66 subsystem checks and the adversarial case pass.
- [x] Resolve strict lint/format causes without broad suppressions or deleting intended supported behavior. Verify fallback catalog counting against current authoritative model data. Format and strict workspace/all-target/all-feature lint pass.
- [x] Run focused regressions, freeze combined source, obtain independent review and run the required unchanged full gate. If verification infrastructure changes, run its intentional-defect self-test and retain restoration evidence. Preserve every failed attempt and inspect actual logs; at most three repairs per specific failure.
- [x] Once the gate and evaluator pass, bump once and build asynchronously. Preserve the old executable by timestamp/SHA, sign/stage/atomically install once, and verify candidate/staged/installed hashes, signature, permissions, ownership, version/help and representative installed native workflows. Update continuity/task/testing records and complete this plan.

## Boundaries and decisions

Only source and disposable state are used for repair. Production configuration, credentials, saved sessions, running user processes, oracle/reference trees and Antigravity evidence remain untouched. Root has no Git metadata or `.agent/PLANS.md`; exact inventories and before-images provide recoverable change evidence. Existing baseline receipts are immutable. All changes are part of restoring this gate; no adjacent features or cleanup are added.

Parallel ownership: one agent owns TUI render/input failure causes, one owns automation resource/Git fixtures, and one owns the three failing CLI integration families. Parent owns shared main/headless wiring, catalog/lint fixes, verification orchestration, records and the single installation. Cargo jobs use one coordinated lane. Freeze delegated edits before final verification copies and request fresh independent review of the combined diff.

## Progress and evidence

- Current baseline captured in `scratch/VAL-288/baseline-source.json` and `before/`. Existing 0.1.39 source identity is checked against the actual current inputs rather than assumed.
- Initial inspection groups the failures into deterministic snapshot version geometry, a chord timeout, disconnected questionnaire/personality paths, native verification evidence, a cross-provider fixture requesting an unavailable Explore tool, host-dependent learning-vault resource samples, and a timestamp-dependent clean-filter assertion. These hypotheses remain subject to focused executed verification.
- Existing questionnaire CLI implementation was already present but unregistered. Its original bytes, aliases, bounds and existing unit test are preserved; only module/dispatch and headless frozen personality prompt wiring were restored. Native headless verification was unconditionally disabled and is now admitted for ordinary persistent, unscheduled Agent runs under existing policy.
- Focused failed attempts remain under `focused-v1/` through `focused-v6/`: changed-file format hunks, the first render result (248 pass / 7 fail), an unsupported custom fixture name, and later assertions exposing four residual header cells (253 pass / 2 fail). The fixture was corrected to builtin Explore; production implicit presets now intersect admitted tools and fail if empty, while explicit spawn/batch grants remain strict. No existing tests were deleted or ignored. Existing preview behavior and 17 inconsistent header spacing cells were reconciled narrowly; two pre-existing catalog expectations were corrected against current source.
- Fresh independent reviews v1–v6 are retained. V1 found a new terminal-cell versus byte-length assertion; fixed. Final v6 accepts the combined result with no findings. Final reviewed root digest: `612be64fc0af2ec6ef4e8068e8997d4ff0e0345ad4cce3c5adefd49b1950e646`. Disposable frozen work digest: `2f29d338ad7ddea60e9cc5183ef2a3291916a7e572c722ae0c30238d003c74ef`. The only input omissions are all 145 pre-existing generated `.snap.new` outputs; every original remains in root/before, and all accepted snapshots/Rust/verification inputs are present. No verifier/evaluator source or strict lint configuration changed.
- `focused-v7/` directly reran the existing compiled final Rust test binary after correcting only four dynamically loaded snapshot header cells: 255 render checks and the new strict explicit-grant/implicit-preset policy regression pass. The full gate rebuilds and verifies the complete final source identity. Other focused passing families are indexed by `focused-v3/` and `focused-v4/`; strict lint/format by `focused-v6/`. Earlier failures remain failed.
- The unchanged full gate has started as job/session 70015 at `scratch/VAL-288/gate-v1/`, launched from the frozen disposable work copy. The user requested immediate scope closure; no optional audits or extra feature work remain. After a passing gate/evaluator, perform one signed release/install and required actual readback, then stop.

## Completion evidence

VAL-288 is complete and installed as signed ca 0.1.40 through one atomic installation (job 61655, exit 0). Full unchanged gate v2 and evaluator pass: default 4317 passed / 0 failed / 43 ignored across 29 targets; all features 4473 / 0 / 44 across 30 targets. Formatting, strict workspace/all-target/all-feature lint, workspace/all-feature release build and all nine native terminal checks pass. Existing ignored cases and all failed receipts remain preserved.

Candidate/staged/installed signed SHA-256: `4660c3687ff50f4f8a2d1f0f05673a5cab8c780246b31c756ebf556b024eda9a`; mode 0755, ownership 501:20, valid-on-disk code signature, normal PATH version/help and installed native readback pass. Candidate and installed 12-column questionnaire screens show the complete version, exit 0 and restore terminal modes; read-only questionnaire CLI checks preserve private state. Prior signed 0.1.39 remains recoverable at `/Users/aji/.local/bin/ca.recovery-VAL-288-20261022T230037Z-acd3dc5b6461947899da3c6d9accb3d2d361e61a8097ef923cb7400286691993`. One post-validation package bump is 39→40; only the two root package version fields differ from the tested/reviewed code.

Evidence: `scratch/VAL-288/gate-v2/`, `gate-v2-assessment.json`, `release/installed-readback.json`, release build/transaction logs, and `final-assessment.json`. Production configuration, credentials, saved sessions and running user processes were untouched. Real-account provider success remains unverified. No VAL-288 work remains.

- Gate v1 preserved at `scratch/VAL-288/gate-v1/`: format/lint/workflow contracts passed; main unit target reached 3440 passed / 1 failed / 42 ignored, and cross-provider CLI all three cases passed. The actual remaining panic was `Transport(Timeout)` at `client.chat_stream`, before trace reading. This content fixture inherited unrelated 40–50 ms timeout-test deadlines. Its local policy now allows a bounded five seconds with one attempt; global policies and deadline tests remain unchanged. A separate trace race was repaired by reading after the writer flush barrier. All protocol/event/session assertions remain intact. The known-failed verifier was interrupted through its owned SIGINT cleanup; its incomplete report/evaluation remains a failure, not a pass.
- Fresh independent review v8 accepts the final delta with no findings. Frozen root digest `28336aa9c2239450fcb3b38979585012ff56e7c3ec5b313151194fd46bb6f07a`; work digest `09b5c09a245a6a502738a66f8039384ff5a6a6d54222aa5a7a225b2ced85dd00`. `work-source-v2.json` records every input. The final trace-focused rerun and unchanged full gate v2 are running in session 52884. No installation or version bump has occurred.

- The final trace-focused check passes (`focused-v8/`, 1 case). The actual debug entry point also passes the real 12-column questionnaire terminal check with full version, exit 0 and terminal restoration (`native-width12-debug/`). Gate v2 setup/workflow/format checks pass; strict lint is running. An unused root compiler incremental cache was removed to make build room; concurrent available space did not increase, so no disk-space recovery is claimed. No source, evidence, recovery binary or runtime state was removed.

- Full default workspace matrix passes in gate v2 (913.532 seconds). Setup, workflow checks, formatting and strict lint also pass. All-features is running; release/native/evaluator and final signed installation remain pending. No criteria were changed.

- Full gate v2 and evaluator completed passed with exit 0; all eight required stages pass. Report/evaluation and raw positive logs were inspected. Assessment: `scratch/VAL-288/gate-v2-assessment.json`. Only root package version metadata advanced once after validation; release source digest `fceab0ef70ccbada0d2e3bf11e1545cef87ba2914a069cb6e5e1a10d5221e412`. Release/install job 61655 is running at `scratch/VAL-288/release/build.log`; no installation success is claimed before its receipt and readback.


## Changed files and final identity

Runtime wiring and policy repairs are in `src/main.rs`, `src/agent/subagent.rs`,
`src/keymap.rs`, `src/host_evidence.rs`, `src/app.rs`, `src/proactive_delegation.rs`,
`src/command_mutations.rs`, `src/provider_filter.rs` and `src/turn_recovery.rs`.
Render/version fixtures and the narrow visible title repair are in
`src/tui/{render,test_support,personality,dashboard,render_tests}.rs` and 13 accepted
snapshot files. Deterministic resource/Git fixtures are in
`crates/ca-automation/src/self_improvement/{vault,daily_budget,learning_loop,supervisor}.rs`,
`crates/ca-automation/src/git.rs`, `src/harness_promotion.rs`,
`src/agent/client_tests.rs` and `tests/cross_provider_subagents_cli.rs`.
`Cargo.toml` and `Cargo.lock` carry the single prescribed patch bump. Exact 37-input
inventory: `scratch/VAL-288/changed-source-files-final.json`; no source files were
added or removed. Harness records and this completed plan are updated.

The reviewed code is root digest
`28336aa9c2239450fcb3b38979585012ff56e7c3ec5b313151194fd46bb6f07a`;
frozen gate work digest
`09b5c09a245a6a502738a66f8039384ff5a6a6d54222aa5a7a225b2ced85dd00`;
post-validation release digest
`fceab0ef70ccbada0d2e3bf11e1545cef87ba2914a069cb6e5e1a10d5221e412`.
Source checks at build/install/readback match that binding. Supplemental strict
lint configuration is unchanged. Only unused reproducible compiler incremental
caches were reclaimed for build room; concurrent available space did not increase,
so no recovered disk capacity is claimed. Source, reports, test binaries, backups,
oracle/reference evidence and user state were preserved.
