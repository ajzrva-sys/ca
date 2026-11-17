# ExecPlan Standard

An ExecPlan is a living implementation document for work that is risky, multi-file, protocol-sensitive, unclear, or likely to outlive one session.

This is the root `ca` standard. `crates/codex-team/.agent/PLANS.md` binds only inside that crate. Older plans that say this file is absent describe the tree before 2026-11-13. FLT-321 already named `ca-hrs-252/.agent/PLANS.md` as the format source; LRN-150 and SEC-158 follow the sections below.

## When to write one

Write an ExecPlan for complex features, significant refactors, migrations, risky changes, multi-file behavior changes, unclear requirements, or work that spans more than one session. In this repo that includes protocol, event-loop, approval, session-format, and parity work.

A small change with one owner, a known focused test, and no protocol, persistence, or parity impact stays a `NEXT_STEPS.md` entry. Link an ExecPlan from `NEXT_STEPS.md`. Do not paste the plan into that queue.

## Required qualities

- Self-contained. A new agent can execute it from the working tree plus the plan.
- Understandable to a novice. Define repo-specific terms in plain language.
- Evidence-backed. Separate source facts, command output, oracle observations, and inferences. Mark inferences `Inferred`.
- Living. Update progress, surprises, decisions, and validation as they happen.
- Executable. Name exact files, commands, tests, and acceptance criteria. Validation must show working behavior, not only a successful compile.
- Safe. Allowed files, forbidden areas, runtime side effects, idempotence, and recovery are explicit.
- Enough to proceed. Implementing agents continue when the plan and the tree answer the question. Resolve ambiguity from repository evidence. Destructive or high-risk operations need explicit human approval. The standing post-task `ca` install rule in `AGENTS.md` is the only install authorization, and a plan must not widen it.

## Storage

- Active: `docs/exec-plans/active/<task-id>-<slug>.md`
- Completed: `docs/exec-plans/completed/<task-id>-<slug>.md`
- Move a plan only after acceptance criteria and the commands that ran are recorded in it.

## Section aliases

New `ca` plans use the headings in the template. These match the prompt-style names:

| This repo | Also satisfies |
|---|---|
| Goal | Purpose / Big Picture |
| Progress | Progress |
| Surprises and Discoveries | Surprises & Discoveries |
| Decisions | Decision Log |
| Evidence and Current Behavior, Scope | Context and Orientation |
| Design and Invariants, Milestones | Plan of Work, Concrete Steps, Interfaces and Dependencies |
| Acceptance Criteria, Validation | Validation and Acceptance |
| Recovery and Rollback | Idempotence and Recovery |
| Completion Record | Artifacts and Notes, Outcomes & Retrospective |

## Template

```markdown
# <Task ID> — <Title>

Status: DRAFT | ACTIVE | BLOCKED | COMPLETE
Owner/session:
Last updated:

## Goal

One observable outcome.

## Non-Goals

Explicitly excluded work.

## Evidence and Current Behavior

- Source facts with paths and symbols.
- Reproduction command and actual output.
- Oracle evidence when parity applies.
- Inferences labeled `Inferred`.
- Definitions of repo-specific terms used below.

## Scope

Allowed files:
Forbidden files/areas:
Runtime state affected:
Interfaces and dependencies:

## Design and Invariants

Data flow, boundaries, protocol, approval, and TUI invariants. Name public APIs, schemas, CLIs, config files, and external systems the change touches.

## Milestones

### M1 — <characterization>
- edits and tests
- expected observable result
- validation command

### M2 — <implementation>
...

## Acceptance Criteria

Pass/fail behaviors with named tests or fixtures. Behavior, not "implemented".

## Validation

Focused:
Completion:
Manual/visual:
Blocked checks:

## Progress

- [ ] durable checklist item

## Decisions

- Decision:
  Evidence:
  Consequence:

## Surprises and Discoveries

Facts that change execution, including failed assumptions.

## Recovery and Rollback

Idempotence: which steps are safe to repeat, and which artifacts must not be rebuilt.
Recovery: how to return to a known state without destructive Git operations.

## Completion Record

Changed files:
Commands and results:
Artifacts (receipt paths, logs):
Remaining risks:
Harness updates:
```

## Execution rules

1. Verify referenced paths and symbols before writing the plan.
2. Begin with a failing characterization test when the behavior already exists.
3. Update `Progress` after each milestone.
4. Record a failed approach when it prevents repetition.
5. Do not claim oracle parity without the fixture `PARITY.md` requires.
6. Run the gates in `TESTING.md`. Record failures as failures.
7. On completion, reconcile `CONTINUITY.md`, `NEXT_STEPS.md`, and architecture or parity docs, then move the plan.
