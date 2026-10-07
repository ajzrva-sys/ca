# Helicon Adoption Readiness

## Purpose / Big Picture
Prepare `codex-team` to adopt Helicon-like SwiftStacker repositories without misreading existing `NEXT_STEPS.md` tasks, running models into a large pre-existing dirty tree, or rejecting deterministic release benchmark commands that are safe when represented as typed recipes.

## Progress
- [x] Inspect current queue parser, dirty-tree, control-doc, and verification-recipe seams.
- [x] Add queue parser support for `## TASK-ID` headings and `Status: Ready`.
- [x] Add large dirty adoption pause and report before reconciliation/model calls.
- [x] Add uppercase `Docs/` control-doc handling and update adoption guidance.
- [x] Add typed SwiftStacker release build/run recipes.
- [x] Add generated-artifact snapshot guardrails without hiding explicitly allowed analysis/script files.
- [x] Add follow-up task for mechanical parity-preflight enforcement.
- [x] Run focused tests and full validation.

## Surprises & Discoveries
- The current queue parser only starts task sections on `### ` headings.
- The existing dirty-tree continuation lane is intentionally limited to small dirty source/test sets and uses `git diff --name-only HEAD`, so it does not see untracked adoption artifacts.
- Verification recipes already enforce no-shell command execution; Swift release mode and `swift run` are simply absent from the typed recipe model.
- On a case-insensitive macOS filesystem, checking both `docs/ROADMAP.md` and `Docs/ROADMAP.md` can return duplicate canonical paths. Control-doc path discovery now dedupes canonicalized candidates.

## Decision Log
- Scope is Readiness Core: no mechanical parity-preflight blocking in this slice.
- A large or mixed adoption dirty tree should produce a Rust-owned report and human-disposition pause before Project Manager, CEO, or Worker is called.
- `Status: Ready` means pending/senior-gateable, not approved/runnable.
- SwiftStacker release benchmark commands must be modeled as typed recipe variants, not prefix allowlists.

## Context and Orientation
- Queue parsing: `src/queue/next_steps.rs`.
- Dirty adoption and orchestration pause: `src/app.rs`.
- Typed verification commands: `src/verification/recipe.rs` plus app bridge helpers still in `src/app.rs`.
- Path/control-doc assumptions: `src/paths.rs`, `src/app.rs`, role prompts, and README.
- Tests: parser tests live in `src/queue/next_steps.rs`; app/recipe/guard tests live in `src/app_tests.rs`.

## Plan of Work
First, extend the pure queue parser so Helicon's existing task shape is visible as pending work without becoming runnable. Next, add a reconciliation preflight that records large/mixed dirty state and reuses the human-disposition pause before any model call. Then update control-doc path recognition, SwiftStacker recipe support, and guard ignored paths. Finally, document the parity-preflight blocker as future work and validate with the requested focused Cargo filters plus the full suite.

## Concrete Steps
1. Patch `src/queue/next_steps.rs` to treat both `## ` and `### ` headings with task IDs as task boundaries, while preserving top-level queue-summary behavior.
2. Add parser tests for Helicon-shaped `DOC-001`, `PAR-001`, `GRD-001`, and `PAR-002` sections.
3. Add dirty adoption summary/report helpers in `src/app.rs` and call them before reconciliation when `state.needs_reconciliation` is true.
4. Add tests that a large mixed dirty set pauses and writes a capped report.
5. Add `Docs/` variants to tracking/control-doc handling and tests.
6. Extend recipe variants/parsing/validation/expansion for Swift release build and SwiftStacker release benchmark run.
7. Extend default guard ignored paths for generated SwiftStacker/Helicon artifact roots and test explicit allowed analysis/script changes remain visible.
8. Update README/planner guidance and add `NEXT_STEPS.md` follow-up for mechanical parity-preflight enforcement.

## Validation and Acceptance
Acceptance criteria:
- Helicon-shaped `## DOC-001` and `Status: Ready` parse as pending and not approved.
- Existing `### ... Status: Approved` behavior remains unchanged.
- A large Helicon-like dirty adoption set pauses before model calls and writes a report with counts, modified tracked files, and capped untracked examples.
- Uppercase `Docs/` control docs are recognized alongside lowercase `docs/`.
- SwiftStacker release build/run commands parse into typed no-shell commands; unsafe variants are rejected.
- Generated artifact roots are ignored by guard snapshots unless explicitly allowed analysis/script paths are changed.

Commands:
- `cargo test queue::next_steps`: passed, 10 tests.
- `cargo test dirty_tree`: passed, 4 tests.
- `cargo test verification`: passed, 13 tests.
- `cargo test paths`: passed, 3 tests.
- `cargo test recipe_parser_converts_legacy_commands_to_typed_recipes`: passed.
- `cargo fmt -- --check`: initially found formatting drift; passed after `cargo fmt`.
- `cargo test`: passed, 135 unit tests plus zero-test main/doc targets.
- `cargo build --release`: passed; refreshed `target/release/codex-team`.

## Idempotence and Recovery
All edits are source/docs/tests only. If a focused test fails, fix the owning subsystem and rerun only that filter before broadening. If two patch/verification iterations fail in the same area, stop and remap the call path before more edits.

## Artifacts and Notes
- Do not initialize or modify `/Users/andrewzimmerman/Downloads/heliconfocus`.
- Dirty adoption reports are runtime artifacts under `.orchestrator/observations/` in target repos, not source-controlled fixtures.

## Interfaces and Dependencies
- No CLI command names change.
- Existing configs continue to deserialize.
- New recipe shapes remain internal Rust enums expanded to `std::process::Command`.
- Runtime behavior changes only during adoption/reconciliation of dirty target repos.

## Outcomes & Retrospective
Helicon-shaped queue sections are now adopted as pending work instead of being invisible or runnable. Large mixed dirty adoption sets now pause before reconciliation/model calls, write a capped Rust-owned report under `.orchestrator/observations/`, and require explicit human disposition. Uppercase `Docs/` control docs are accepted alongside lowercase `docs/`, SwiftStacker release benchmark commands are represented as typed no-shell recipes, and generated SwiftStacker/Helicon artifact roots are ignored by default guard snapshots while explicitly scoped analysis/script files remain visible.

Mechanical parity-preflight enforcement remains intentionally out of this slice and is tracked as `ORCH-HEL-002` in `NEXT_STEPS.md`.
