# ORCH-WATCH-002: Stale Verification Hint Suppression

## Purpose / Big Picture
The efficiency watcher should not merely notice large prompts after they happen. It should help prevent one repeated inefficiency: stale observation-memory verification hints. When a hint such as `swift test --filter TiledRendererTests` fails in Rust evidence, future verification plans should skip that hint until a task explicitly asks for it or the project records it as passing again.

## Progress
- [x] Investigate how failed observation hints are represented in evidence.
- [x] Suppress failed local/global observation hints during verification-plan assembly.
- [x] Surface stale hint records in `codex-team watch`.
- [x] Add regression tests.
- [x] Run focused and full validation.
- [x] Move this plan to completed with evidence.

## Surprises & Discoveries
- Helicon evidence records the repeated `TiledRendererTests` command with `command.source = "local_memory.verification_hint"`, `category = "commandFailed"`, and `success = false`.
- The existing `TESTING.md` known-failure skip is insufficient when the failed command lives only in `.orchestrator/evidence/*.jsonl`.
- Target repos may have legacy task evidence JSONL entries shaped as per-command receipts rather than full `EvidenceReport` objects; the stale-hint scanner must parse both without warning spam.

## Decision Log
- Suppress only commands whose prior failed evidence source is `local_memory.verification_hint` or `global_memory.verification_hint`. Explicit task verification commands remain authoritative.
- Keep the stale-hint report advisory in `watch`; do not mutate `verification_hints.json` automatically.
- Dedupe stale-hint alerts by semantic command fingerprint and keep the latest failure so repeated failures do not produce a noisy report.

## Context and Orientation
- Verification plan assembly: `src/app.rs` near `build_verification_plan_with_observation_hints`.
- Evidence shape: `EvidenceReport` and `CommandEvidence` in `src/app.rs`.
- Watcher report: `build_efficiency_watch_report`, `efficiency_watch_suggestions`, and `render_efficiency_watch_report` in `src/app.rs`.
- Tests live in `src/app_tests.rs`.

## Plan of Work
Read recent evidence JSONL records, collect failed observation-hint command semantic fingerprints, and use them as a skip set when appending local/global memory hints. Add watcher alerts that summarize the stale commands and suggest quarantining or refreshing the hint source.

## Concrete Steps
1. Add helper functions to read stale observation-hint failures from `.orchestrator/evidence/*.jsonl`.
2. Wire the stale fingerprint set into `build_verification_plan_with_observation_hints`.
3. Add `stale_verification_hint` alerts to the efficiency watcher.
4. Add tests for plan suppression and watcher reporting.
5. Validate with focused tests, formatting, full tests, release build, and `git diff --check`.

## Validation and Acceptance
- `cargo test stale_verification_hint`
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance:
- A failed local/global-memory observation hint in evidence is not added to a later plan.
- The skipped command appears in `plan.rejected_commands`.
- `codex-team watch` reports stale verification hints with task/source/category evidence.
- No explicit task verification command is suppressed by this mechanism.

## Idempotence and Recovery
The patch is read-only against target repo evidence and does not delete hint indexes. If suppression is too aggressive, remove or narrow the evidence helper and rerun tests. Re-running `watch` overwrites only `.orchestrator/observations/efficiency_watch.*` in the target repo.

## Artifacts and Notes
Helicon evidence showed the inefficient command in `.orchestrator/evidence/PAR-003.jsonl` with source `local_memory.verification_hint`.

Focused validation:
- `cargo test stale_verification_hint`: passed.
- `cargo test recent_failed_observation_hint`: passed.
- `cargo test efficiency_watch`: passed, 3 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 159 tests.
- `cargo build --release`: passed.
- `git diff --check`: passed.
- Live `target/release/codex-team watch --repo /Users/andrewzimmerman/Downloads/heliconfocus`: passed; reported prompt bloat, PM/Worker double-context, repeated `PAR-003`, and deduped stale hints for `TiledRendererTests` and `BenchmarkSuiteTests` with no legacy evidence parse warnings.

## Interfaces and Dependencies
- Uses existing evidence JSONL format.
- Does not add CLI flags or schema files.
- Does not change deterministic command execution.

## Outcomes & Retrospective
`codex-team` now treats failed observation-memory commands as stale evidence. Future verification plans skip matching local/global memory hints and record the skip in `rejected_commands`, while `watch` reports deduped stale hints so the operator can refresh or quarantine them. The release binary at `target/release/codex-team` was rebuilt after validation.
