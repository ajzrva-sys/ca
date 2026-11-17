# Recurring Prompts

These are maintenance recipes, not permission to skip `AGENTS.md` or task scope.

## Repository Re-Survey

> Inspect primary source, manifest, tests, current Git state, and the harness. Do not modify code. Update `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, and `NEXT_STEPS.md` only where command/source evidence changed. Mark inference. Treat `oracles/` and `reference/` as read-only. Report stale claims removed, commands run, and unresolved conflicts.

Use after major merges or when the handoff is visibly stale.

## Close One Parity Slice

Use this only when the current task asks for an oracle comparison. Matching Cursor Agent CLI/TUI behavior is not the product goal.

> Choose one `PARITY.md` row named by the task. Create an ExecPlan. Capture the requested behavior with version, terminal dimensions, exact inputs, and text-only evidence containing no bundled source or secrets. Add a failing deterministic `ca` fixture, make the smallest change, run `TESTING.md` gates, and update only that row. Stop if patched and clean oracle evidence disagree.

## Validation Discovery

> Inspect the changed subsystem and current tests. Identify the smallest focused checks plus required completion gates. Do not run network/provider/user-state tests by default. If a missing deterministic seam blocks validation, add a `NEXT_STEPS.md` task or ExecPlan rather than claiming coverage.

## Guardrail Review

> Review the last confirmed defect or repeated review finding. Write the smallest regression that reproduces it, map it to `docs/QUALITY.md`, and strengthen one check. Avoid repository-wide refactors. Record false-positive risks and the exact failure message expected.

## Documentation Gardening

> Check harness links, stale status claims, path references, and duplicated rules against source and command output. Preserve durable decisions, remove obsolete history, keep `NEXT_STEPS.md` executable, and keep `CONTINUITY.md` as current state rather than a changelog. Do not rewrite source or oracle files.

## ExecPlan Handoff

> Read `.agent/PLANS.md`, the active plan, `CONTINUITY.md`, and relevant source/tests. Verify the last completed milestone from artifacts/commands before continuing. Update progress and discoveries during work. Do not infer completion from old agent notes. Plans that say `.agent/PLANS.md` is absent predate the 2026-11-13 restoration.

## Create an ExecPlan

> Read `AGENTS.md`, `.agent/PLANS.md`, `CONTINUITY.md`, and the owning files in `ARCHITECTURE.md`. Write one plan under `docs/exec-plans/active/` with the root template. Name allowed files, a focused `scripts/check-change.py` selector or a blocked check, recovery, and idempotence. Link it from `NEXT_STEPS.md`. Do not edit product source in this pass. Stop if Cursor behavior cannot be observed.

## Implement an ExecPlan

> Execute the active plan's next unchecked milestone. Stay inside its allowed files. Update Progress, Surprises, and Validation in the plan before stopping. Run the plan's focused command. A full `./scripts/verify.sh` runs only when the plan or `TESTING.md` requires it.

## Review current work

> Run `REVIEW_CHECKLIST.md` against the diff. Cite receipts. Send each finding to the destination table in that checklist. Do not rewrite unrelated docs.

## Feedback into durable knowledge

> Take one repeated review comment, failed command, or contradicted doc sentence. Put a current-state correction in `CONTINUITY.md`, a single follow-up in `NEXT_STEPS.md` or a new ExecPlan, and a guardrail line in `docs/QUALITY.md` when a test or script could catch it. Add a `docs/GOLDEN_PRINCIPLES.md` entry only with a file or test citation.

## Entropy pass

> Compare `docs/.INDEX.md`, `TESTING.md`, and `CONTINUITY.md` with `Cargo.toml` version and `ca --version` only if a release is already installed. Archive completed-plan narration out of the index. Keep links to `docs/exec-plans/active/`. Do not edit plan bodies or source. Record commands and any conflict left in place.
