# ExecPlan Standard

Use an ExecPlan for complex features, significant refactors, migrations, risky changes, multi-file behavior changes, unclear requirements, or work expected to span multiple milestones.

ExecPlans are living documents. They must be self-contained and understandable to a novice with only the current working tree and the plan file. Define repo-specific terms in plain language. Name exact files, commands, tests, and acceptance criteria. Update the plan as progress, surprises, decisions, and validation results occur.

Implementing agents should not ask for next steps when the plan provides enough information. Resolve ambiguities from repository evidence when possible. Destructive or high-risk operations require explicit human approval. Every plan must include idempotence and recovery guidance.

Store active plans under `docs/exec-plans/active/`. Move completed plans to `docs/exec-plans/completed/` when done.

## Required Template

# <Plan Title>

## Purpose / Big Picture
Explain what changes for the user, operator, developer, or system.

## Progress
Use checkboxes and update them as work proceeds.

## Surprises & Discoveries
Record unexpected repo facts, failed assumptions, and new constraints.

## Decision Log
Record decisions with rationale and evidence.

## Context and Orientation
Name exact files, modules, commands, tests, and concepts.

## Plan of Work
Describe the sequence of edits in prose.

## Concrete Steps
Give exact commands and file operations.

## Validation and Acceptance
Define observable acceptance criteria and exact verification commands.

## Idempotence and Recovery
Explain how to retry safely and recover from partial progress.

## Artifacts and Notes
Record concise evidence, outputs, screenshots, logs, or links to generated artifacts where relevant.

## Interfaces and Dependencies
Name public APIs, schemas, CLIs, config files, external systems, or dependencies.

## Outcomes & Retrospective
Summarize final behavior, validation evidence, and follow-up work.
