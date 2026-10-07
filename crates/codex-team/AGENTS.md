# Repository Agent Instructions

## Purpose
- `codex-team` is a local Rust CLI orchestrator for CEO, Project Manager, Senior Dev, and Worker Codex roles.
- Rust owns state transitions, queue parsing, guard checks, deterministic command validation/execution, evidence, telemetry, and commits.
- AI role output is advisory unless converted into typed Rust state, task cards, evidence, or commits.

## Control Rules
- Do not change source code unless a task or ExecPlan explicitly allows it.
- Keep changes small, local, and tied to `NEXT_STEPS.md` or an active ExecPlan.
- Preserve existing docs and behavior unless repo evidence proves they are stale.
- Never invent commands, parity requirements, models, tests, or architecture facts.
- Do not run destructive commands, deployment commands, migrations, or install commands without explicit approval.

## Start Here
1. `docs/.INDEX.md`
2. `ARCHITECTURE.md`
3. `docs/AGENT_MAP.md` — choose the owning file before broad searches or `app.rs` edits.
4. `TESTING.md`
5. `CONTINUITY.md`
6. `NEXT_STEPS.md`
7. `.agent/PLANS.md`
8. `docs/QUALITY.md`

## Source of Truth
1. Primary repo source, manifests, tests, schemas, prompts, and CLI definitions.
2. `README.md` and root control docs.
3. Existing task/roadmap/spec docs.
4. Inferred architecture from layout/imports, marked as inferred.
5. Human instructions in the current task.

If sources conflict, record the conflict in `CONTINUITY.md` and add a concrete `NEXT_STEPS.md` task.

## Work Pattern
- Inspect first with `rg`, `git ls-files`, and focused `sed`; avoid broad file-by-file rereads.
- Plan complex or risky work in an ExecPlan before editing.
- Edit minimally and keep module ownership boundaries intact.
- Validate with commands from `TESTING.md`; mark skipped validation explicitly.
- Update `CONTINUITY.md`, `NEXT_STEPS.md`, or relevant docs when repo truth changes.
- Stop on unsafe ambiguity instead of guessing.

## ExecPlans
- Use an ExecPlan for significant refactors, multi-file behavior changes, migrations, risky changes, unclear requirements, or work spanning multiple milestones.
- Follow `.agent/PLANS.md` exactly.
- Store active plans in `docs/exec-plans/active/`.
- Move completed plans to `docs/exec-plans/completed/` with validation evidence.

## Validation
- Use `TESTING.md` as the command source of truth.
- Minimum for Rust source changes: focused test when possible, then `cargo test`; add `cargo fmt -- --check` when formatting may be affected.
- For docs-only changes, run no code tests unless useful; at least audit links/paths manually and record commands run.
- Do not claim validation passed unless it actually ran and passed.

## Guardrails
- Read `docs/QUALITY.md` before changing orchestration, queue, verification, guard, or Codex-runner behavior.
- Convert recurring mistakes into tests, lints, scripts, docs, or `NEXT_STEPS.md` guardrail tasks.

## Stop Conditions
- Stop if unrelated source files must change.
- Stop if requirements conflict with source/test evidence.
- Stop if a task needs secrets, credentials, network access, or missing local environment.
- Stop before destructive Git, filesystem, deploy, migration, or install commands.
- Stop if validation cannot run and the risk is more than docs-only.

## Done Criteria
- Changed files are in allowed scope.
- Docs and source-of-truth files remain consistent.
- Commands run and skipped validation are recorded.
- Unknowns are documented.
- `NEXT_STEPS.md` contains executable follow-up work when needed.
- Final response names changed files, validation, risks, and first next task.
