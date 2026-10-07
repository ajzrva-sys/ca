# Golden Principles

## Rust Owns Authority

- Principle: Rust owns state transitions, queue status updates, deterministic evidence, guard checks, command execution, and commits.
- Evidence: `README.md` role model and deterministic verification sections.
- Why it matters: model output stays advisory and reviewable.
- Apply it: do not let role prompts or role outputs mutate durable state
  directly; PM and Senior may authorize typed commit requests, but Rust performs
  staging and commits.
- Future enforcement: tests around acceptance, queue updates, and model-skip telemetry.
- Exceptions or unknowns: none verified.

## Task Cards Are the Work Boundary

- Principle: Worker receives one approved task with allowed files, forbidden actions, verification commands, and stop conditions.
- Evidence: `README.md` queue sections and `TaskSpec` in `src/domain/task.rs`.
- Why it matters: prevents broad scans and unauthorized edits.
- Apply it: update `NEXT_STEPS.md` with concrete scoped tasks instead of broad prose.
- Future enforcement: parser tests plus guard report tests.
- Exceptions or unknowns: conditional approved branches are allowed only when Rust can evaluate exact outputs.

## Parse Before Executing

- Principle: legacy command strings must become typed recipes or typed commands before execution.
- Evidence: `README.md` deterministic verification description and verification tests.
- Why it matters: avoids shell injection and keeps evidence structured.
- Apply it: never add raw shell execution as a shortcut.
- Future enforcement: command validator tests and review checklist.
- Exceptions or unknowns: `codex exec` process launch is a separate adapter path.

## Keep Parsers Pure

- Principle: Markdown parsing should not read/write files, run commands, or call Codex.
- Evidence: `src/queue/next_steps.rs` module docs and tests.
- Why it matters: pure parser behavior is easy to test and safe for agents to modify.
- Apply it: keep IO/state wrappers in `app` or a future queue IO module.
- Future enforcement: structural lint or test.
- Exceptions or unknowns: none verified.

## Support Packets Replace Intern Work

- Principle: task briefs, queue snapshots, failure summaries, and review assists are Rust-generated support packets.
- Evidence: README and `legacy_intern_support_contract` tests.
- Why it matters: avoids spending model tokens on deterministic summarization.
- Apply it: update support structs/tests instead of reintroducing an AI Intern role.
- Future enforcement: no Intern prompt/schema generation and no Intern runner calls.
- Exceptions or unknowns: legacy config fields remain deserializable.
