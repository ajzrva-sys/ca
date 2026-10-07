# Repo-Specific Prompts

## Create an ExecPlan

Read `AGENTS.md`, `docs/.INDEX.md`, `ARCHITECTURE.md`, `TESTING.md`, `CONTINUITY.md`, and `.agent/PLANS.md`. Inspect only the files directly related to the requested change. Write a self-contained ExecPlan under `docs/exec-plans/active/` with exact files, commands, validation, idempotence, and stop conditions. Do not edit source code.

## Implement an ExecPlan

Read the active plan, `AGENTS.md`, and `TESTING.md`. Follow the plan exactly, update progress and surprises as you work, run the listed validation, and move the plan to `docs/exec-plans/completed/` only after acceptance criteria are met.

## Review Current Branch

Review the current diff for behavior regressions, architecture-boundary violations, unsafe command paths, missing tests, stale docs, and task/plan drift. Lead with findings and cite file/line evidence. Do not make edits unless explicitly asked.

## Convert Feedback Into Durable Knowledge

Given review comments or repeated agent mistakes, decide whether each item belongs in a test, lint/script proposal, `docs/QUALITY.md`, `docs/GOLDEN_PRINCIPLES.md`, `NEXT_STEPS.md`, `CONTINUITY.md`, or an ExecPlan. Prefer mechanical guardrails over prose-only rules when practical.

## Documentation Gardening

Check `docs/.INDEX.md`, root docs, subsystem READMEs, and `NEXT_STEPS.md` for stale links, stale command status, completed tasks, and duplicated content. Keep edits small. Do not rewrite broad docs without evidence. Record unresolved uncertainty in `CONTINUITY.md`.

## Entropy / Garbage Collection

Find dead or stale docs, completed active plans, broken references, obsolete task status, and commands that no longer exist. Archive completed ExecPlans, update `CONTINUITY.md`, and create small cleanup tasks instead of broad rewrites.

## Validation Harness Discovery

Inspect manifests, README, scripts, CI config, and tests. Update `TESTING.md` and `TOOLS.md` with commands as verified, inferred, or not run. Do not invent commands and do not install tools without approval.

## Guardrail Proposal

Identify one recurring risk that can become a test, lint, script, CI rule, or review checklist item. Add a concrete entry to `docs/QUALITY.md` with invariant, evidence, enforcement method, failure message, priority, and related task.
