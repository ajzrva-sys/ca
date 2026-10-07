# Quality Guardrails

## Current Gates

| Gate | Command | Protects | Current limitation |
|---|---|---|---|
| default workspace tests | `cargo test --workspace --no-fail-fast` | package/unit/integration behavior | does not replace native PTY or external-service checks |
| full workspace tests | `cargo test --workspace --all-targets --all-features --no-fail-fast` | feature/target combinations | platform-specific behavior still needs native hosts |
| formatting | `cargo fmt --all -- --check` | stable Rust formatting | broad formatting rewrites remain review-gated |
| strict lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | common correctness/style issues | cannot establish runtime/provider behavior |
| release build | `cargo build --workspace --release --all-features` | both optimized binaries | reproducibility needs two clean fixed-epoch builds |
| snapshots | absence of `*.snap.new` plus reviewed diffs | deterministic wide/narrow TUI states | cannot validate terminal modes, PTYs, or cleanup |
| readiness receipt | `codex-team verify release-readiness --repo . --json` | tracked inputs and canonical release evidence | remains not-ready until the dirty integration is tracked |

The pinned root workflow mirrors these gates locally. It is not “CI-backed”
until committed to a remote and observed passing there. See `TESTING.md` for
actual results and manual checks.

## Guardrails

### GRD-001 — Hermetic provider protocol test

Trigger: tool/client/orchestration changes.  
Rule: fake transport must assert system/user/assistant/tool ordering, IDs, multi-call batches, denial, errors, cancellation, and malformed streaming data.  
Failure message should name the call ID and expected/actual message sequence.

### GRD-002 — Deterministic ratatui snapshots

Trigger: render, app state, input overlay, markdown, diff, status, picker, or streaming changes.  
Rule: render with `TestBackend` at wide and narrow dimensions; cover Unicode and no-color assumptions. Normalize time, paths, and random IDs.  
Snapshot updates require human review and a parity citation when behavior claims Cursor equivalence.

### GRD-003 — UTF-8 cursor property/table tests

Trigger: composer/vim/paste/path-completion changes.  
Rule: every editing operation leaves the cursor at a character boundary and never panics for representative emoji, combining, CJK, and multiline inputs.

### GRD-004 — Async responsiveness test

Interrupt regression (TUI-240): aborting a foreground tool waiter must reap its
process group, not merely stop its UI future. Cover promotion/ownership isolation,
late-event rejection, held queues, and visible stop controls under transient
status. Busy Enter must never silently become queueing again.

Trigger: event-loop, slash side effect, provider, or tool execution changes.  
Rule: ticks/cancellation progress while a controlled slow task is pending. Reject new `block_on`/`block_in_place` paths reachable from TUI orchestration unless narrowly justified and tested.

### GRD-005 — Approval exactly-once model

Trigger: approval or tool-batch changes.  
Rule: approved, denied, remembered, errored, and cancelled calls each resolve once in original order, and continuation occurs once per completed batch.

### GRD-006 — Oracle write boundary

Trigger: any task using `oracles/` or `reference/`.  
Rule: final status/diff inspection must show no changes there. Oracle source/build artifacts may not be added to Rust dependencies or copied into fixtures.

### GRD-007 — Persistence isolation

Trigger: config, sessions, context, shell history, custom agents, or undo changes.  
Rule: tests use temporary directories and explicit injected roots. No test reads/writes user state.

### GRD-008 — Child admission and terminal ownership

Trigger: subagent spawn, batch, lifecycle handle, detached completion, or
global pause changes.
Rule: validate before admission, serialize pause/correlation decisions, bind
terminal events to one origin/turn, and prove accepted work is either delivered
or represented by one truthful terminal receipt.

### GRD-009a — Behavior-contract tests (Hermes-informed)

Trigger: new tests around prompts, compaction, host OS, or session stores.
Rule: assert observable behavior contracts (role alternation, byte-stable
system prompt when appropriate, checkpoint user-quote preservation) rather than
snapshotting full prompt text. Never read product source inside tests; never
fake host OS (use real `#[cfg]`); ban change-detector tests that only restate
implementation names.

### GRD-009 — Recoverable managed-asset mutation

Trigger: automatic or multi-step skill curation and child spill persistence.
Rule: use private bounded roots, exact digests, atomic writes, and staged
recovery tests for every filesystem transition. Foreign skill sources remain
read-only and automatic mutation defaults off.

## Failure Handling

- Do not auto-format the whole repository merely to pass the gate; scope and review formatting debt explicitly.
- Do not weaken a lint or approval policy to make a test pass.
- Do not convert a live-provider failure into a fake success. Separate hermetic regressions from opt-in integration checks.
- If a guardrail is too noisy, improve its fixture/scope before suppressing it.

## Maintenance Loop

After a recurring defect:

1. write the smallest failing regression;
2. decide which guardrail should have caught it;
3. strengthen that guardrail or add a proposed one;
4. update `TESTING.md` and relevant module docs;
5. remove obsolete warnings once enforcement exists.
