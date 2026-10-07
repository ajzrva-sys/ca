# IDX-250 — Index Cursor-Informed Draft Plans

Status: COMPLETE (docs-only; executed 2026-09-17)
Owner/session: Cursor study session 2026-09-17
Last updated: 2026-09-17

## Goal

Produce one durable harness index of the Cursor-informed DRAFT ExecPlans
created this session so a future executor can see scope, order, and
non-goals without rediscovering chat history.

## Non-Goals

- Do not implement CUR-245 / SEC-246 / TUI-247 / CMD-248 / SES-249 features.
- Do not edit `oracles/`, Cursor install trees, or `src/`.
- Do not claim Cursor runtime parity.

## Evidence and Current Behavior

Study bench: Cursor Agent CLI `agent-cli-local` `2026.09.15-d2fe57e` via
`agent-local-{deepseek,mimo}` (no Cursor account; local providers).
DRAFT plans written under `docs/exec-plans/active/`; `NEXT_STEPS.md` heads
link each one. `docs/.INDEX.md` still lacked a single grouped index for them.

## Scope

Allowed files:

- `docs/exec-plans/active/IDX-250-cursor-informed-draft-plan-index.md` (this plan)
- `docs/exec-plans/completed/IDX-250-cursor-informed-draft-plan-index.md` (move on complete)
- `docs/CURSOR_INFORMED_PLAN_INDEX.md` (canonical index)
- `NEXT_STEPS.md` (IDX-250 entry + point at canonical index)
- `docs/.INDEX.md` (link the canonical index)

Forbidden: `oracles/`, `src/`, credentials, unrelated active plan bodies.

## Design and Invariants

- Index only; child plans stay DRAFT until explicitly activated.
- One canonical index file; other docs link to it.
- Record cloud-vs-local boundary as a non-goal summary for all children.

## Milestones

### M1 — Write canonical index
- `docs/CURSOR_INFORMED_PLAN_INDEX.md` listing five DRAFT plans + evidence paths.

### M2 — Wire harness docs
- `NEXT_STEPS.md` IDX-250 entry; `docs/.INDEX.md` link.

### M3 — Close this plan
- Move to `completed/`; Progress checked.

## Acceptance Criteria

- [x] Canonical index exists and lists all five DRAFT plans with paths.
- [x] `NEXT_STEPS.md` and `docs/.INDEX.md` reference the index.
- [x] No `src/` or `oracles/` changes.
- [x] Child plans remain DRAFT.

## Validation

Docs-only: file presence + link paths resolve; `git status` shows only allowed docs.

## Progress

- [x] M1 canonical index
- [x] M2 harness wiring
- [x] M3 complete + move plan

## Decisions

- Decision: Index-only execution; do not start child implementation.
  Evidence: User asked to index then execute *this* plan.
  Consequence: Five feature plans stay DRAFT.

## Completion Record

Changed files: see index + harness links.
Commands and results: docs-only.
Remaining risks: none for code; child plans still unimplemented by design.
Harness updates: `NEXT_STEPS.md`, `docs/.INDEX.md`, `docs/CURSOR_INFORMED_PLAN_INDEX.md`.
