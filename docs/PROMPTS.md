# Recurring Prompts

These are maintenance recipes, not permission to skip `AGENTS.md` or task scope.

## Repository Re-Survey

> Inspect primary source, manifest, tests, current Git state, and the harness. Do not modify code. Update `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, and `NEXT_STEPS.md` only where command/source evidence changed. Mark inference. Treat `oracles/` and `reference/` as read-only. Report stale claims removed, commands run, and unresolved conflicts.

Use after major merges or when the handoff is visibly stale.

## Close One Parity Slice

> Choose one `PARITY.md` row. Create an ExecPlan. Capture clean Cursor behavior with version, terminal dimensions, exact inputs, and text-only evidence containing no bundled source/secrets. Add a failing deterministic `ca` fixture, make the smallest change, run `TESTING.md` gates, and update only that parity row. Stop if patched and clean oracle evidence disagree.

## Validation Discovery

> Inspect the changed subsystem and current tests. Identify the smallest focused checks plus required completion gates. Do not run network/provider/user-state tests by default. If a missing deterministic seam blocks validation, add a `NEXT_STEPS.md` task or ExecPlan rather than claiming coverage.

## Guardrail Review

> Review the last confirmed defect or repeated review finding. Write the smallest regression that reproduces it, map it to `docs/QUALITY.md`, and strengthen one check. Avoid repository-wide refactors. Record false-positive risks and the exact failure message expected.

## Documentation Gardening

> Check harness links, stale status claims, path references, and duplicated rules against source and command output. Preserve durable decisions, remove obsolete history, keep `NEXT_STEPS.md` executable, and keep `CONTINUITY.md` as current state rather than a changelog. Do not rewrite source or oracle files.

## ExecPlan Handoff

> Read `.agent/PLANS.md`, the active plan, `CONTINUITY.md`, and relevant source/tests. Verify the last completed milestone from artifacts/commands before continuing. Update progress and discoveries during work. Do not infer completion from old agent notes.
