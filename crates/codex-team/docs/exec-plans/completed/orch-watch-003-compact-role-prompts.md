# ORCH-WATCH-003: Compact Role Prompt Budgets

## Purpose / Big Picture
Helicon progress is safe, but the watcher keeps reporting expensive Project Manager and Worker prompts for routine queue-empty planning and docs-only execution slices. Compact mode exists, yet the configured excerpt budgets are still large enough to send long control docs. Tighten compact PM/Worker prompt budgets so higher-level AI can continue dictating tasks without burning repeated 11k-12k prompts.

## Progress
- [x] Identify the prompt-size source.
- [x] Lower compact Project Manager and Worker control-doc budgets.
- [x] Add prompt-size regression tests.
- [x] Run focused and full validation.
- [x] Rebuild the release binary.
- [x] Move this plan to completed with evidence.

## Surprises & Discoveries
- `planner_context = "compact"` is already the default.
- Compact Project Manager still includes up to 10k chars of roadmap, 14k chars of `NEXT_STEPS.md`, and 10k chars of non-AI queue content.
- Worker-like prompts include up to 8k chars of roadmap, 12k chars of `NEXT_STEPS.md`, and 6k chars of non-AI queue content even though the selected task and queue snapshot are already in `Extra run context`.

## Decision Log
- Keep higher-level PM task generation enabled. This is not a move to manual queues.
- Reduce context budgets rather than changing role responsibilities.
- Preserve full mode for explicit broad reconciliation via `planner_context = "full"`.

## Context and Orientation
- Prompt rendering lives in `src/app.rs` near `PromptLimits`, `render_prompt_limited`, and `render_project_manager_compact_prompt`.
- PM role calls are built in `run_project_manager`.
- Worker role calls are built in `run_worker`.
- Watcher alerts live near `build_efficiency_watch_report`.

## Plan of Work
Add smaller compact prompt limits, use them for PM compact mode and Worker-like calls, and test that oversized control docs produce prompts below the 10k-token watcher threshold by a comfortable character proxy.

## Concrete Steps
1. Add `PromptLimits::compact_planner`.
2. Tighten `PromptLimits::worker_like`.
3. Use compact planner limits in `render_project_manager_compact_prompt`.
4. Add regression tests for compact PM and Worker prompt sizes with large control docs.
5. Validate and rebuild `target/release/codex-team`.

## Validation and Acceptance
- `cargo test compact_prompt`
- `cargo test efficiency_watch`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Acceptance:
- Compact PM prompts use queue brief plus short excerpts, not full control-doc bodies.
- Worker prompts stay centered on the selected task and support brief.
- Full planner mode remains available for broad reconciliation.
- Release binary is refreshed.

## Idempotence and Recovery
Prompt budgets are static constants. If this becomes too terse, raise the compact budget constants and rerun prompt-size tests. No target repo product files are changed by this plan.

## Artifacts and Notes
Watcher evidence from Helicon after PAR-005:
- Project Manager prompt was ~12,102 tokens.
- Worker prompt was ~11,676 tokens for a docs-only task.
- PM and Worker ran back-to-back with large prompts.

Focused validation:
- `cargo test prompt_stays`: passed, 2 tests.
- `cargo test efficiency_watch`: passed, 3 tests.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed, 161 tests.
- `cargo build --release`: passed.

## Interfaces and Dependencies
- No CLI changes.
- No config schema changes.
- Existing `planner_context = "full"` remains the escape hatch.

## Outcomes & Retrospective
Compact Project Manager and Worker role prompts now use smaller control-doc budgets. Higher-level AI can still generate the next parity task from the mission and queue brief, but routine planning/execution calls should no longer inherit large `ROADMAP.md`, `NEXT_STEPS.md`, and `NEXT_STEPS_non-ai.md` excerpts. The release binary at `target/release/codex-team` was rebuilt.
