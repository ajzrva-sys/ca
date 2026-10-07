# Codex Team Project Instructions

This repository is managed by a four-role Codex orchestrator plus a Rust
support layer:

CEO -> Project Manager -> Senior Dev -> Worker

## Shared rules

- Treat `ROADMAP.md` as the high-level source of truth.
- Treat `NEXT_STEPS.md` as Project Manager's compiled work-order queue and
  Senior Dev's review log.
- Treat `NEXT_STEPS_non-ai.md` as the deterministic Rust queue for
  verification/read-only tasks that should not spend model tokens.
- Treat `AGENTS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`, and
  `Docs/.INDEX.md` or `docs/.INDEX.md` as control documents that exist to save future tokens.
- Prefer small, commit-sized changes.
- Keep work moving toward the mission; avoid open-ended busy work.
- When editing code, run the narrowest relevant checks that prove the task.
- Do not ask the human for routine engineering decisions. Escalate through the role protocol instead.

## Cost / reasoning hierarchy

- CEO is the expensive high-reasoning role. Use it for mission direction,
  roadmap corrections, strategic risk calls, and completion decisions.
- Project Manager is the high-reasoning sequencing layer. It converts CEO
  direction and repo state into roadmap milestones and safe ticket queues.
- Senior Dev is the technical safety layer. It constrains tickets, dispatches
  approved work, verifies allowed files, reviews output, and authorizes Rust to
  commit accepted or already-bounded work.
- Worker is the bounded coding executor. It receives one approved task with
  exact files, steps, checks, and stop rules.
- Rust support packets provide deterministic task briefs, queue snapshots,
  failure summaries, review assists, and safe command evidence. They are
  advisory context only; they do not approve work, decide roadmap direction, or
  replace role judgment.
- Always simplify work before spending higher-tier tokens: send command-only
  verification/read-only tasks to `NEXT_STEPS_non-ai.md`, deterministic edits
  to Worker, task sequencing to Project Manager, technical constraints/review
  to Senior Dev, and only true strategic ambiguity or repeated plan failure to
  CEO.

## Git tracking

- Worker tasks must not commit.
- Project Manager task-compiler tasks must not implement or run git directly;
  they may return typed Rust commit requests for already-dirty bounded slices.
- Senior Dev review should inspect the diff, authorize Rust to create a Git
  commit when the task is accepted or technically ready, and reject instead of
  editing source when a repair needs Worker. Do not fix trivial issues by editing
  files during review; keep Senior advisory and let Rust own commits.
- CEO and Project Manager should not directly commit implementation work.

## Control documents

CEO or Project Manager may create and update repo-derived Markdown control docs
when they reduce future agent scanning. Senior Dev may update only task status,
review notes, and narrow hygiene notes while dispatching approved work. Core docs are
`AGENTS.md`, `CONTINUITY.md`, `ROADMAP.md`, `NEXT_STEPS.md`, `TESTING.md`,
`ARCHITECTURE.md`, and `Docs/.INDEX.md` or `docs/.INDEX.md`. Conditional docs such as `TOOLS.md`,
`ENVIRONMENT.md`, `DECISIONS.md`, `KNOWN_ISSUES.md`, `SECURITY.md`,
`RELEASE.md`, `REVIEW_CHECKLIST.md`, or `specs/.INDEX.md` should exist only
when the repo clearly needs them. Do not create empty, speculative, or filler
docs.
