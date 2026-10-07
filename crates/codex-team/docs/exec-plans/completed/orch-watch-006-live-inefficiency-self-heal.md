# ORCH-WATCH-006: Live Inefficiency Self-Heal

## Purpose / Big Picture
Make `codex-team watch --self-heal` useful during live Helicon-style adoption runs by reporting current unresolved inefficiency instead of hiding it behind a stale self-heal baseline, and by reducing Senior review prompt size for bounded evidence tasks.

## Progress
- [x] Reproduce the watcher masking issue from the current Helicon run.
- [x] Patch watcher alert selection so recent prompt bloat is visible.
- [x] Compact Senior review context without weakening Rust-owned verification.
- [x] Add regression tests.
- [x] Validate and rebuild the release binary.
- [x] Move this ExecPlan to completed with evidence.

## Surprises & Discoveries
- `watch --self-heal` reported `alerts: none` even though the latest Helicon cost records show prompt estimates of 10,179 tokens for Project Manager, 10,288 for Worker, and 13,739 for Senior Dev review.
- The existing self-heal timestamp baseline can hide later prompt bloat even when the self-heal action is `already_applied` and no actual repair occurred.

## Decision Log
- Treat prompt-efficiency alerts as recent-call signals, not a permanently baseline-suppressed history query. This keeps old resolved bloat from lingering while still surfacing new regressions.
- Keep AI role output advisory. Senior still receives Worker output, guard, Rust verification, and compact queue context, but the Worker output is converted to a bounded Rust-owned receipt before insertion into the Senior prompt.

## Context and Orientation
- Watcher code lives in `src/app.rs` near `build_efficiency_watch_report`, `efficiency_prompt_alerts`, `efficiency_adjacent_role_alerts`, and `apply_efficiency_self_heal`.
- Role prompt assembly lives in `src/app.rs` near `run_project_manager`, `run_worker`, `run_senior_dev_review`, and `PromptLimits`.
- Tests live in `src/app_tests.rs` near the efficiency watcher and compact prompt tests.
- The user runs `/Users/andrewzimmerman/project/codex_team_orchestrator/target/release/codex-team`, so a release rebuild is required after source changes.

## Plan of Work
Patch the watcher to inspect the most recent non-skipped role calls before applying prompt-bloat thresholds. Remove blanket timestamp suppression from prompt and adjacent-role alerts. Add a compact Senior review Worker receipt and use tighter Senior review control-doc limits. Update tests that previously encoded the blanket self-heal baseline behavior, and add coverage for no-op self-heal not hiding later bloat.

## Concrete Steps
1. Edit `src/app.rs` watcher alert helpers and Senior review prompt construction.
2. Edit `src/app_tests.rs` watcher and prompt-size regression tests.
3. Run focused tests: `cargo test efficiency_watch`, `cargo test compact_`, and any new filter.
4. Run `cargo fmt -- --check`, `cargo test`, `cargo build --release`, and `git diff --check`.
5. Move this plan to `docs/exec-plans/completed/` and record validation evidence.

## Validation and Acceptance
- `watch --self-heal` flags current post-baseline prompt bloat instead of reporting no alerts.
- A no-op self-heal report with `already_applied` does not hide newer prompt bloat.
- Old prompt bloat ages out after recent clean calls.
- Senior review prompt assembly uses a bounded Worker receipt.
- Focused and full Cargo validation pass.
- `target/release/codex-team` is rebuilt.

## Idempotence and Recovery
All changes are local Rust source, tests, and docs. Re-running `watch --self-heal` only writes advisory observation files in the target repo. If validation fails, inspect the failing helper/test first and avoid broad changes outside watcher/prompt assembly.

## Artifacts and Notes
- Current Helicon attachment: PAR-007 cycle showed PM ~10.1k, Worker ~10.2k, Senior ~13.7k.
- Local reproduction: `codex-team watch --repo /Users/andrewzimmerman/Downloads/heliconfocus --self-heal` returned `alerts: none`, proving the mask.
- After the patch and release rebuild, `watch --repo /Users/andrewzimmerman/Downloads/heliconfocus --self-heal` reports prompt-bloat alerts for PAR-007 Senior, PAR-007 Worker, and the preceding Project Manager call. A second watch run without `--self-heal` still reports those alerts, proving the no-op self-heal report no longer masks the issue.

## Interfaces and Dependencies
- CLI surface: `codex-team watch --self-heal`.
- Observation files: `.orchestrator/observations/efficiency_watch.*`, `.orchestrator/observations/efficiency_self_heal.*`.
- Cost ledger: `.orchestrator/cost.jsonl`.

## Outcomes & Retrospective
Implemented. Prompt alerts now inspect the most recent non-skipped role calls instead of hiding all records before an `efficiency_self_heal.json` timestamp. A no-op self-heal can no longer suppress later prompt-bloat alerts. Senior Dev review now receives a bounded Worker receipt and tighter control-doc excerpts through `PromptLimits::senior_review()`.

Validation passed:
- `cargo test efficiency_watch`
- `cargo test compact_`
- `cargo test self_heal`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`
