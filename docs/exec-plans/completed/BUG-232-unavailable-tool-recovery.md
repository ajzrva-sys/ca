# BUG-232 — Bounded Unavailable-Tool Recovery

Status: COMPLETE (implemented; AUT-231 integration blockers cleared under BUG-234)
Owner/session: Codex `/root`
Last updated: 2026-09-03

## Goal

Keep an interactive turn usable when an OpenAI-compatible provider emits one
tool call outside the exact advertised set: reject the call without execution,
return a safe tool error to the same logical turn, and allow one provider
continuation before failing closed on repetition.

## Non-Goals

- Do not widen Main, Plan, Ask, custom-mode, workflow, CLI-selected, MCP, or
  model capability tool sets.
- Do not execute an unadvertised tool, silently change the process tool policy,
  add provider-specific tool aliases, or retry without a model-visible result.
- Do not change headless, service, review, subagent, workflow, or saved-session
  schemas in this repair.
- Do not contact a real provider, mutate runtime configuration/sessions, or
  install a binary during validation.

## Evidence and Current Behavior

- `src/main.rs` records the exact prepared names in
  `AppState::active_tool_names`, then checks streamed tool names at
  `StreamMsg::Finish`.
- A mismatch currently preserves partial assistant text, fails the turn, and
  shows `Provider attempted unavailable tool: <name>. Adjust the tool selection
  and use /retry`.
- `search_files` is a registered read-only built-in in
  `src/agent/registry.rs`; it is omitted only when profile, mode, workflow, or
  process tool-selection constraints remove it.
- The exact official DeepSeek model profile declares tool support, and its Chat
  continuation path already replays provider-required reasoning state.
- Inferred: the provider produced a structurally valid tool call that did not
  obey the request's advertised schema set. Executing it would widen authority;
  terminating immediately makes a recoverable model mismatch user-blocking.

## Scope

Allowed files:

- `src/main.rs` and focused tests in that module
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`,
  `docs/.INDEX.md`, and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, provider/tool/session schemas, approval or
  permission rules, user runtime state, credentials, installed binaries, and
  unrelated dirty-worktree changes

Runtime state affected:

- One interactive logical turn may make at most one additional provider
  continuation after a rejected unadvertised tool call. Usage remains recorded
  normally. No new persistent state is introduced.

## Design and Invariants

- Compare every completed streamed call against the exact names advertised for
  that request. Calls outside the set become resolved `ToolStatus::Error`
  results with a bounded, argument-free correction message; they never enter
  approval, hooks, registry dispatch, or tool execution.
- Preserve advertised calls in the same batch as ordinary `Running` calls and
  execute only those unresolved calls.
- Detect prior recovery from the typed resolved result already present after
  the current user-turn boundary, avoiding a new session field or schema.
- If all calls were unavailable, continue directly to the provider with the
  resolved error result. If calls are mixed, finish the advertised subset and
  continue through the existing exactly-once resolution path.
- On a second unavailable call in the same logical turn, retain the existing
  fail-closed behavior and `/retry` boundary. Workflows retain their current
  explicit failure/retry semantics and do not use this interactive recovery.
- Never include tool arguments, provider payloads, endpoints, credentials, or
  hidden state in the correction result or status.

## Milestones

### M1 — Characterize the mismatch

- Add focused tests for first-mismatch correction, mixed-batch filtering, and
  repeat detection.
- Confirm the tests fail against the current terminal behavior.

### M2 — Implement bounded recovery

- Add pure classification/result helpers and route `StreamMsg::Finish` through
  them.
- Filter already-resolved calls out of the execution batch and use the existing
  continuation path when no executable calls remain.

### M3 — Validate and reconcile

- Run focused tests, then formatting, full tests, strict Clippy, and release as
  required by `TESTING.md`, recording any failures caused by the pre-existing
  dirty worktree separately.
- Update the harness and move this plan to completed only after the required
  observable checks pass.

## Acceptance Criteria

- A first unadvertised interactive tool call is represented as a resolved error
  and is never executed or approved.
- The provider receives at most one automatic continuation and can complete the
  same logical turn without the user entering `/retry`.
- A repeated unadvertised call fails closed with the existing safe error and no
  third request.
- A mixed batch executes only advertised calls; the unadvertised call remains a
  resolved error in canonical history.
- Explicit tool selection, profiles, mode restrictions, workflows, approvals,
  workspace containment, sandboxing, and network authority remain unchanged.

## Validation

Focused:

- `cargo test -p ca unavailable_tool -- --nocapture`: PASS, 4/4.
- `cargo test -p ca mixed_tool_batch_leaves_only_advertised_calls_executable
  -- --nocapture`: PASS, 1/1.
- `cargo test -p ca tool_selection::tests -- --nocapture`: PASS, 17/17.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA
  2,384/4 ignored; codex-team 652/1 ignored; MCP stdio 8/8; all other targets
  and doc tests pass.
- `cargo build -p ca`: PASS with the two pre-existing AUT-231 dead-code
  warnings and the known non-fatal macOS compact-unwind linker warning.
- `git diff --check`, snapshot hygiene, and direct oracle/reference status:
  PASS.
- `cargo build --workspace --release --all-features`: PASS after the explicitly
  requested cache repair. `cargo clean --release` removed 11,222 generated
  files/5.7 GiB, and the fresh normal parallel build finished in 3m03s. The
  release passes version/help and strict code-signature smokes; SHA-256 is
  `6e97b2388c413a649bada43952ffe2791c545a8d0d8621553a698876f5bd1eb8`.

Manual/visual: source screenshot characterized; no real provider call planned.

Blocked checks:

- `cargo fmt --all -- --check`: pre-existing paused AUT-231 formatting
  differences in `src/automation.rs`, `src/schedule.rs`, `src/session.rs`, and
  the existing `src/main.rs` module order. The unrelated dirty files were not
  broadly formatted.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pre-existing unused AUT-231 `AutomationStore::claim_due` and
  `AutomationExecution::successful_tools` only.

## Progress

- [x] Traced interactive preparation, exact advertised-name tracking, finish
  validation, tool batching, and provider continuation.
- [x] Confirmed `search_files` exists and DeepSeek's exact model contract
  supports tools.
- [x] Added failure-first focused regression coverage.
- [x] Implemented bounded recovery without authority widening.
- [x] Cleared unrelated AUT-231 format/Clippy blockers (done under BUG-234:
  formatter drift fixed and the two unused AUT-231 items removed, so
  `cargo fmt --all -- --check` and strict all-feature Clippy now pass).

## Decisions

- Decision: return a normal tool error to the model instead of adding the tool
  or silently retrying the raw request.
  Evidence: the call is syntactically valid but outside the exact request
  authority; existing continuation supports resolved tool errors.
  Consequence: the hidden tool cannot execute, while the model gets enough
  context to self-correct.
- Decision: allow exactly one recovery per logical user turn.
  Evidence: compatible providers can repeat a hallucinated call, and automatic
  provider work must remain bounded.
  Consequence: one transient mismatch is recoverable; repetition remains a
  visible terminal failure with `/retry` available.

## Surprises and Discoveries

- The screenshot's exact error originates after successful provider streaming,
  not from DeepSeek rejecting the request or from a missing local handler.

## Recovery and Rollback

Reverse only the focused patch with `apply_patch`; do not use destructive Git
commands. The existing terminal fail-closed branch remains the fallback for a
repeat and can be restored without changing schemas or runtime data.

## Completion Record

Changed files: `src/main.rs`, focused harness pages, queue/index, and this plan.

Commands and results: focused and complete workspace tests pass; diff,
snapshot, and protected-tree hygiene pass. Formatting and strict Clippy are
blocked only by the pre-existing paused AUT-231 changes listed above. Release
passes after the explicitly requested clean rebuild of generated artifacts.

Remaining risks: no live DeepSeek request was made. The deterministic tests
exercise the provider-independent post-stream and execution boundaries; a live
provider smoke remains optional once credentials and runtime mutation are
separately authorized.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`,
`NEXT_STEPS.md`, and `docs/.INDEX.md` describe the repair and gate state.
