# REVIEW_CHECKLIST

Use this before accepting agent-produced changes.

## Correctness

- Does the change match the task or ExecPlan exactly?
- Are source-of-truth docs, schemas, prompts, and tests consistent?
- Did the agent avoid inventing commands or behavior?

## Architecture Boundaries

- Queue parser changes stay pure.
- Verification changes preserve typed command validation and evidence.
- Guard changes remain based on Rust snapshots and allowed files.
- Codex process changes go through the adapter seam.
- Role changes return decisions; they do not commit or mutate queue state directly.

## Minimal Diff

- Are unrelated files untouched?
- Is `src/app.rs` reduced or kept stable rather than expanded casually?
- Are docs updated only where they add operational leverage?

## Tests and Validation

- Were commands from `TESTING.md` run?
- Are skipped commands recorded with reasons?
- Did focused tests run before broad `cargo test` for source changes?

## Reliability and Safety

- No raw shell execution bypasses typed validation.
- No destructive Git/filesystem commands were used.
- No secrets, credentials, or local machine assumptions were added.

## Durable Knowledge

- Add recurring mistakes to `docs/GOLDEN_PRINCIPLES.md` or `docs/QUALITY.md`.
- Add concrete follow-up work to `NEXT_STEPS.md`.
- Update `CONTINUITY.md` for durable decisions, unknowns, and changed repo state.
- Use an ExecPlan for unresolved multi-step work.
