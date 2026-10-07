# BTW-046 — Ephemeral Side Questions

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-10

## Goal

Add `/btw [question]` as a process-only side-question overlay that remains usable during an active main turn, freezes the largest protocol-complete canonical context at invocation, performs one independent tool-free provider request, and never changes canonical messages or persisted sessions.

## Non-Goals

- Do not run tools, hooks, MCP discovery, subagents, approvals, queued input, or session mutation for a side question.
- Do not include live partial assistant output, pending pasted images, composer drafts, hook context, or queued messages in the frozen context.
- Do not interrupt, pause, steer, retry, or otherwise change the active main turn.
- Do not persist side questions, answers, errors, or display state. Usage accounting remains process-only through the existing ledger.
- Do not support more than one concurrent side question, fallback providers, background notification delivery, or claim Cursor parity.
- Do not modify oracle/reference trees, real user configuration, Keychain data, saved sessions, or the installed binary.

## Evidence and Current Behavior

- `src/tui/commands.rs` distinguishes `Always` commands from idle-only commands; the slash palette and `queue_command` already allow `Always` commands while a main turn is active.
- `src/main.rs::start_llm_turn` owns the main provider task, dynamic MCP catalog, built-in/custom tools, hooks, streaming state, and the main cancellation handle. Reusing it would violate BTW isolation.
- `AgentClient::chat_stream_with_options` omits the serialized `tools` field when passed an empty slice, and `agent::stream::consume_stream` provides bounded typed stream completion and usage.
- `session::completed_turns` identifies completed assistant exchanges but excludes an otherwise protocol-valid in-flight user message. BTW needs a narrower protocol helper that preserves complete user input while removing only an unresolved tool suffix.
- `AppState.messages` contains canonical messages; partial provider output is held separately in `AppState.streaming`, so cloning a validated canonical prefix excludes live partial text by construction.
- `usage::UsageKind::Btw` already exists, but no current request records that category.
- Oracle review found no clean Cursor fixture for `/btw`. This is an intentional oracle-informed `ca` extension.

## Scope

Allowed files:

- `src/{app,context,main,usage}.rs`
- `src/tui/{commands,input,input_tests,render,render_tests}.rs`
- source-owned focused tests and snapshots
- `docs/exec-plans/{active,completed}/`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`

Forbidden files/areas:

- `oracles/`, `reference/`, real config/Keychain/session roots, installed binaries
- provider protocol, normal main-turn retry/tool-loop semantics, or session schema

Runtime state affected:

- One process-only BTW overlay, independent background task/generation, and process-only usage record.
- No persistent state.

## Design and Invariants

1. `/btw` is visible, `Always`, and accepts an optional question. An argument starts immediately; no argument opens an isolated editor. Input and paste never touch the normal composer while the overlay is open.
2. Invocation freezes the current provider/model, reasoning options, system prompt components, and largest protocol-complete canonical request prefix. A complete current user message remains eligible; live `streaming` content and any unresolved assistant tool-call suffix are excluded.
3. The request appends the side question only to a task-local message vector. It passes an empty tool slice, never reads/clears hook context or pasted images, and never calls MCP catalog, hook, tool, approval, agent, queue, checkpoint, or session paths.
4. `BtwState` contains only question, streamed answer, typed status, safe provider/model IDs, cursor/scroll, and generation. It never contains resolved credentials, endpoint, headers, request messages, system prompt, or session objects.
5. A dedicated task handle and generation isolate BTW from the main stream. Esc while running aborts only BTW, records cancellation once, and leaves a visible cancelled state; stale messages cannot mutate a newer/closed overlay or task handle.
6. A valid completed stream with no tool calls becomes Complete. Typed provider/protocol failures stay inline and secret-free. Generated reasoning and tool calls are not displayed or retained; output text is never copied into canonical messages.
7. Exactly one `UsageKind::Btw` span starts before provider I/O and closes as completed, failed, or cancelled. Closing the overlay discards the question/answer while the process ledger remains.
8. Main-turn completion/failure/cancellation and normal session saving proceed independently while BTW is open. Side state never participates in session construction.

## Milestones

### M1 — Characterization and frozen-context boundary

- Add command tests proving palette discovery, optional arguments, `Always` availability, uniqueness, and unchanged busy behavior for idle-only commands.
- Add focused tests for the largest protocol-complete prefix: normal history, current user suffix, resolved tool exchange, unresolved tool exchange, and canonical immutability.
- Add process-only BTW state and editor/input tests, initially failing before orchestration exists.

Expected result: BTW input and context selection are deterministic and isolated before provider traffic is added.

Validation: focused command/context/input tests.

### M2 — Independent request orchestration and display

- Add the dedicated task/generation/stream events and tool-free request runner.
- Integrate typed completion/failure/cancellation, usage, safe stale-result rejection, and shutdown cleanup.
- Add the modal renderer and reviewed wide/narrow snapshots for editing, running, completed, failed, and cancelled states.
- Add loopback tests proving exactly one request, no tools, no partial output/hooks/MCP/session mutation, independent main streaming, safe failure, usage, cancellation, and stale generations.

Expected result: a user can ask and dismiss a side question during a main turn without affecting that turn or persistent conversation state.

Validation: focused BTW loopback/orchestration/render suites and snapshot review.

### M3 — Completion gates and reconciliation

- Run full tests, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Exercise the rebuilt TUI with a temporary home and loopback provider: start a delayed main turn, open `/btw`, complete one side question, cancel another, let the main turn complete, and prove the saved session contains neither side question nor answer.
- Reconcile architecture, continuity, testing, next steps, and parity; record actual results and move this plan to completed before starting KEY-047.

Expected result: BTW-046 is independently releasable and the next roadmap plan remains inactive until every gate passes.

## Acceptance Criteria

- `/btw` and `/btw <question>` are visible and usable while the main turn is active; only one side question can run.
- The side request contains a frozen protocol-complete canonical prefix plus exactly one ephemeral user question, excluding live partial output, unresolved tool suffixes, composer/queue drafts, hook context, MCP schemas, tool schemas, and images.
- Serialized side requests contain no `tools` field and cause no tool, hook, MCP, approval, subagent, queue, checkpoint, or session action.
- Esc cancellation affects only BTW; normal main cancellation remains unchanged and both tasks can finish in either order.
- Complete, failed, cancelled, and stale-result paths close exactly one BTW usage record with safe provider/model provenance.
- Closing/reopening discards side content; session saves, reloads, forks, retries, and canonical message snapshots contain no side question or answer.
- Wide/narrow editing/running/complete/failure/cancelled render states remain bounded and readable.
- All focused, repository-wide, release/smoke, snapshot, and temporary-home walkthrough gates pass without real runtime side effects.

## Validation

Focused:

- `cargo test btw_ -- --nocapture`: PASS, 9 command/input/loopback/orchestration/render tests.
- Protocol-prefix characterization: PASS for normal history, current submitted user message, resolved tools, unresolved suffix trimming, and canonical immutability.

Completion:

- `cargo test --no-fail-fast`: PASS, 498 unit tests plus 25 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/bare-profile smoke: PASS (`ca 0.1.0`).

Manual/visual:

- Ten wide/narrow editing/running/complete/failure/cancelled snapshots reviewed; zero `.snap.new` files remain.
- Temporary-home rebuilt TUI: delayed main request, completed BTW request, cancelled BTW request, independent main completion, and clean exit observed. The saved session contained only `MANUAL-MAIN`/`MAIN-ANSWER-SAVED`; both side markers were absent. Config SHA-256 remained `d0809c2dfa530066ce5b79a9fb1896226ab85a4571ab65118917ed309b2ccc4f`; the server and temporary root were removed.

Blocked checks: none.

## Progress

- [x] Existing command, stream, usage, session, and overlay seams inspected.
- [x] Active ExecPlan created before source edits.
- [x] Failing characterization and frozen-context tests added.
- [x] Independent state/input/request orchestration complete.
- [x] Wide/narrow snapshots reviewed.
- [x] Full gates and temporary-home walkthrough complete.
- [x] Harness reconciled and plan ready to archive.

## Decisions

- Decision: define completed context as the largest protocol-complete canonical prefix, rather than only completed assistant turns.
  Evidence: a currently submitted user message is complete and useful context even while its assistant answer streams; partial answer state is stored outside canonical messages, while unresolved tool exchanges can exist inside canonical messages.
  Consequence: BTW can understand the active request without seeing partial model output or sending an invalid tool protocol transcript.
- Decision: use Esc as BTW-only cancellation and preserve existing Ctrl+C main-turn cancellation semantics.
  Evidence: Ctrl+C is already the emergency cancellation path for the active main turn.
  Consequence: task ownership is unambiguous and existing safety muscle memory remains intact.
- Decision: retain only final content, not reasoning or tool calls, in BTW display state.
  Evidence: BTW is a short tool-free answer surface and reasoning/tool metadata is unnecessary process state.
  Consequence: the overlay stays bounded in purpose and cannot accidentally advertise an unavailable tool action.

## Surprises and Discoveries

- A completed submitted user message is already safe provider context while its assistant response lives only in `StreamingState`; trimming to completed assistant turns would unnecessarily hide the active question. The prefix helper therefore stops only at the first unresolved tool-bearing message.
- The temporary cleanup safety layer rejected recursive removal. The two known temporary files were deleted explicitly and every now-empty directory was removed with exact `rmdir` targets.

## Recovery and Rollback

- Remove the BTW command/state/input/render/task/events and focused tests with targeted patches. No config/session migration or runtime data rollback exists because all new state is process-only.
- Abort the dedicated BTW task on close/shutdown; never use destructive Git operations to recover.

## Completion Record

Changed files: `src/{app,context,main}.rs`, `src/tui/{commands,input,render,render_tests}.rs`, ten BTW snapshots, this ExecPlan, and the five harness documents.

Commands and results: all focused, full, format, strict-lint, release, smoke, snapshot, and isolated TUI gates above passed.

Remaining risks: side-answer display state is process-only and uses the shared bounded SSE event/parser/deadline controls; there is intentionally no second side question, tool use, fallback, or persistence in this slice.

Harness updates: architecture request/cancellation invariants, continuity inventory/verification/handoff, testing evidence, next-step status, and intentional-extension parity classification reconciled.
