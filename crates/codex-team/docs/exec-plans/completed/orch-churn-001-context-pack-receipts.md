# Token Churn Context Pack Receipts

## Purpose / Big Picture
Make token churn visible before changing more routing policy. Worker prompts should be built from named sections with budgets, caps, priorities, and a durable receipt. Retry prompts should use compact attempt capsules instead of full prior transcripts, full logs, or full diffs.

## Progress
- [x] State intended contract and minimal slice.
- [x] Add `ContextPackBuilder` and prompt receipt types.
- [x] Build Worker prompts through named context sections and hard caps.
- [x] Add Attempt Capsule retry context.
- [x] Persist prompt receipts and reject over-budget Worker prompts before model launch.
- [x] Add tests for receipts, budget rejection, and retry capsule log/diff suppression.
- [x] Validate and rebuild release binary.

## Surprises & Discoveries
- `src/codex/prompt_pack.rs` exists as an empty target home for context-pack construction.
- The Codex adapter already records total prompt token estimates, but not section-level receipts.

## Decision Log
- Keep PM read-only in this slice. The new measurement layer will inform later PM-vs-Worker/Rust routing policy instead of reintroducing PM mutation immediately.
- Scope the first receipt builder to Worker prompts, where token churn is most expensive and retry loops are most visible.

## Context and Orientation
- `src/app.rs` builds Worker prompts in `run_worker`.
- `src/codex/run.rs` records total prompt token estimates in `.orchestrator/cost.jsonl`.
- `src/codex/prompt_pack.rs` is the target module for prompt pack construction.
- `src/repair.rs` defines `WorkerRetryBrief`; this slice will derive an attempt capsule from that brief.

## Plan of Work
Implement a pure context-pack builder that receives named sections and produces both prompt text and a prompt receipt. Wire Worker prompt construction through it, with separate sections for role prompt, task text, repo hints, file contents, logs, diff, previous attempts, and guidelines. Persist receipts under `.orchestrator/observations/prompt_receipts/`. Add compact Attempt Capsule support for retries and stop before Worker if the built pack exceeds the hard token budget.

## Concrete Steps
1. Add context pack and prompt receipt structs/helpers in `src/codex/prompt_pack.rs`.
2. Add config defaults for Worker prompt hard token budget.
3. Replace Worker prompt rendering with a sectioned context pack.
4. Add Attempt Capsule derivation from `WorkerRetryBrief`.
5. Persist receipts/events before Worker launch and reject over-budget prompts before calling the runner.
6. Add focused tests and update docs with validation.

## Validation and Acceptance
- `cargo test context_pack`
- `cargo test prompt_receipt`
- `cargo test worker_prompt`
- `cargo fmt -- --check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

## Idempotence and Recovery
Receipts are append-only runtime observations. If a prompt is over budget, Rust writes the receipt and returns an error before any model process is launched. Retrying after config/budget changes is safe.

## Artifacts and Notes
This is orchestrator-only. Do not run commands in Helicon during implementation.

## Interfaces and Dependencies
- New receipt files live under `.orchestrator/observations/prompt_receipts/`.
- Worker prompt construction starts using `ContextPackBuilder`.
- The existing cost ledger remains intact; receipts add section-level detail.

## Outcomes & Retrospective
Completed. Worker prompts now use named, capped context sections with durable `PromptReceipt` JSON. Rust writes receipts before Worker launch, prints the token estimate and rough context-review time, and rejects over-budget Worker prompts without calling Codex. Retries now derive compact Attempt Capsules from `WorkerRetryBrief`, and repeated failure fingerprints omit duplicate full logs and diffs. `ROADMAP.md` and `NEXT_STEPS.md` now put token-churn reduction ahead of additional agent-orchestration work.

Validation:
- `cargo test context_pack`: passed 2 tests.
- `cargo test prompt_receipt`: passed 1 test.
- `cargo test worker_prompt`: passed 3 tests.
- `cargo test attempt_capsule`: passed 1 test.
- `cargo fmt -- --check`: passed.
- `cargo test`: passed 187 tests.
- `cargo build --release`: passed.
