# Review checklist

Use this before accepting an agent change in the root `ca` tree. Inside `crates/codex-team/`, use that crate's `REVIEW_CHECKLIST.md` and `.agent/PLANS.md`.

## Correctness

- The diff matches the claimed `NEXT_STEPS.md` task or ExecPlan.
- Source, tests, and harness docs describe the same behavior.
- Commands and parity claims come from `TESTING.md`, `PARITY.md`, or a receipt. Invented success is a reject.

## Boundaries

- `oracles/` and `reference/` are unchanged.
- Tool schema, execution name, approval class, and tool-result message stay aligned.
- `python3 scripts/check-pyramid.py` rejects a new `block_on` or `block_in_place` under `src/tui/`.
- Approval checks are not weaker. A new destructive slash command has an explicit scope and a test.
- Session format, protocol, and approval changes name the broader gate they require.

## Diff size

- Unrelated files are untouched.
- Formatting edits stay inside the change that needed them.
- Docs change where ownership, commands, parity, or handoff state changed.

## Validation

- A Rust behavior change has a focused test and a `python3 scripts/check-change.py --selector …` receipt, or a recorded reason that command could not run.
- `./scripts/verify.sh` is cited only when its report exists for this change.
- Skipped network, provider, and user-state checks are named.
- A visible TUI change has deterministic render coverage. A live progress change was watched while still pending.

## Oracle notes

- A task that asks for an oracle comparison cites the `PARITY.md` row and the fixture.
- Codex and Hermes notes are labeled as engineering lessons.
- Oracle source was not copied in.

## Durable knowledge

- `CONTINUITY.md` matches the installed version and any new conflict.
- `NEXT_STEPS.md` links an ExecPlan instead of duplicating it.
- A repeated mistake becomes a test or a `docs/QUALITY.md` guardrail proposal.
- A durable invariant that is not already in `docs/GOLDEN_PRINCIPLES.md` is proposed there only with a file or test citation.

## Where a finding goes

| Finding | Destination |
|---|---|
| Wrong current behavior or install fact | `CONTINUITY.md` |
| One local follow-up | `NEXT_STEPS.md` |
| Multi-session or risky follow-up | `docs/exec-plans/active/` plus a queue link |
| Recurring defect | test, then `docs/QUALITY.md` |
| Durable invariant | `docs/GOLDEN_PRINCIPLES.md` |
| Stale command or broken harness link | `docs/PROMPTS.md` gardening pass |
