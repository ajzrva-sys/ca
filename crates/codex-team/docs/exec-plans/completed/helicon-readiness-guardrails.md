# Helicon Readiness Guardrails

## Purpose / Big Picture
Finish the orchestrator-only safety work needed before using `codex-team` on a Helicon-like SwiftStacker checkout. This slice keeps Helicon itself untouched and adds Rust-owned blocking around renderer/campaign preflight, SwiftStacker benchmark report roots, uppercase `Docs/` routing, generated analysis artifacts, and adoption dry-run behavior.

## Progress
- [x] Inspect current ORCH-HEL-001 worktree and relevant orchestration seams.
- [x] Add mechanical parity-preflight blocking before Worker/deterministic execution.
- [x] Add SwiftStacker benchmark report-root overwrite protection.
- [x] Finish uppercase `Docs/` cleanup in prompts/docs/tracking helpers.
- [x] Extend generated analysis artifact ignore defaults.
- [x] Add adoption dry-run fixture and focused tests.
- [x] Run focused tests, full validation, and release build.

## Surprises & Discoveries
- ORCH-HEL-001 is present in the worktree but not committed.
- Existing task policy validation is the right pre-Worker seam for the preflight gate.
- Existing typed SwiftStacker recipes already constrain report roots to `Benchmarks/Reports*`; this slice needs filesystem overwrite checks.
- Broad `analysis/*.csv` ignores would hide explicitly allowed analysis files, so generated analysis ignores are directory-scoped instead.

## Decision Log
- Do not inspect, initialize, or modify `/Users/andrewzimmerman/Downloads/heliconfocus` during this implementation.
- Treat renderer/campaign-affecting work by path/scope classification, not Helicon-specific absolute paths.
- Keep report-root overwrite protection in Rust verification policy rather than in shell command parsing.

## Context and Orientation
- Orchestration and policy checks live mostly in `src/app.rs`.
- Tests live in `src/app_tests.rs`.
- Typed recipe shape lives in `src/verification/recipe.rs`, with parsing/validation bridge code in `src/app.rs`.
- Docs and prompts need `Docs/` case handling updates.

## Plan of Work
First, add pure helpers for task scope classification and preflight packet detection. Then call the guard before Worker or deterministic task execution can proceed. Next, add report-root overwrite checks for SwiftStacker benchmark recipes at verification-plan execution time. After that, finish `Docs/` path cleanup and generated artifact ignore defaults. Finally, add tests for each guardrail and complete the validation pass.

## Concrete Steps
1. Patch `src/app.rs` with preflight detection, task-scope classification, and blocking status.
2. Patch verification command validation to reject existing SwiftStacker report roots unless task scope explicitly allows them.
3. Add `Docs/ROADMAP.md` and `Docs/.INDEX.md` to remaining tracking-doc helper paths and prompts.
4. Extend default guard ignores for generated `analysis/` roots while preserving explicitly allowed analysis/script files.
5. Add focused tests for `parity_preflight`, report-root overwrite protection, uppercase `Docs/`, generated analysis ignores, and Helicon-shaped adoption dry run.

## Validation and Acceptance
Acceptance criteria:
- Renderer/campaign-affecting tasks without a seven-field preflight in `CONTINUITY.md` or an active ExecPlan are blocked before Worker.
- Non-renderer docs/planning tasks are not blocked.
- Existing SwiftStacker report roots are rejected unless the exact root is explicitly allowed.
- Uppercase `Docs/` is treated consistently where lowercase `docs/` was already accepted.
- Generated analysis artifact roots are ignored by default guard snapshots, but explicitly allowed `analysis/*.md`, `analysis/*.csv`, and `Scripts/*.py` remain visible.
- A Helicon-shaped adoption dry run pauses before any fake role call and writes the dirty adoption report.

Commands:
- `cargo test parity_preflight`: passed, 3 tests.
- `cargo test helicon_adoption_dry_run`: passed.
- `cargo test verification`: passed, 15 tests.
- `cargo test dirty_tree`: passed, 4 tests.
- `cargo test paths`: passed, 3 tests.
- `cargo test guard_ignores_generated_swiftstacker_artifacts_but_not_analysis_scripts`: passed.
- `cargo test tracking_doc_completion_notes_include_uppercase_docs_variants`: passed.
- `cargo fmt -- --check`: initially found formatting drift; passed after `cargo fmt`.
- `cargo test`: passed, 142 unit tests plus zero-test main/doc targets.
- `cargo build --release`: passed; refreshed `target/release/codex-team`.
- `git diff --check`: pending final hygiene check.

## Idempotence and Recovery
All edits are orchestrator source/docs/tests only. If a focused test fails, fix the owning policy seam and rerun that filter before broader validation. If two patch/verification iterations fail in the same area, stop and remap the call path.

## Artifacts and Notes
- No commands should run in `/Users/andrewzimmerman/Downloads/heliconfocus`.
- `target/release/codex-team` should be rebuilt after source validation.

## Interfaces and Dependencies
- No public CLI command names change.
- Existing task cards remain compatible.
- New policy behavior is a pre-execution block, not a new role or shell path.

## Outcomes & Retrospective
The orchestrator now blocks renderer/campaign-affecting tasks before Worker unless a seven-field `Renderer/Campaign Preflight` packet exists in `CONTINUITY.md` or an active ExecPlan. SwiftStacker benchmark runs reject existing `Benchmarks/Reports*` report roots unless the task explicitly allows that root. Uppercase `Docs/` control-doc handling is covered in tracking-note helpers and prompts. Generated analysis directories are ignored by default guard snapshots without hiding explicitly allowed analysis markdown/CSV or script files. A synthetic Helicon-shaped adoption dry run proves the first step pauses before fake role calls and writes the dirty adoption report.

No commands were run in `/Users/andrewzimmerman/Downloads/heliconfocus`.
