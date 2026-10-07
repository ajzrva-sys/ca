# TUR-078 — Soft Pause

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add `/pause` and `/unpause` so an active interactive turn can stop at the next completed provider or tool boundary without cancelling work, losing canonical state, starting hidden tools/provider continuations, or conflating the feature with durable goal/workflow pause controls.

## Non-Goals

- Interrupting an in-flight provider request, tool, hook, approval, or interaction; pausing headless/scripted/review/checkpoint/subagent runners; persisting a soft pause in Session V4.
- Replacing Ctrl+C cancellation, queue/steer behavior, goal/workflow pause state, provider retry, terminal job control, or the later durable `/term` feature.
- Cursor parity. This is an oracle-informed `ca` extension without a clean Cursor fixture.

## Evidence and Current Behavior

- `src/activity.rs::TurnLifecycle` distinguishes one active turn from terminal completion/cancellation but has no paused phase or continuation boundary.
- `src/main.rs` starts tool execution immediately after a provider `Finish` (or response hooks) through `start_completed_tool_batch`. After tool results and manual resolutions, `continue_after_tool_resolutions` immediately starts the next provider request.
- Tool batches preserve assistant tool calls and then converge through `ManualResolutionQueue`; provider continuations therefore have an exact safe gate after all calls have outputs. The provider completion path has an exact safe gate after the assistant/tool-call message and response hooks have completed but before any tool starts.
- `/goal pause` and `/workflow pause` are durable owner-specific controls. Their tests and state must remain unchanged. Ctrl+C currently terminates the active lifecycle and must remain cancellation.
- Busy slash filtering uses `CommandAvailability::Always`; `/pause` and `/unpause` must therefore be explicit Always commands to remain reachable while a turn is active.
- Baseline `cargo test activity` passed 2 tests and `cargo test pause` passed 8 tests before edits.

## Scope

Allowed files: `src/activity.rs`, `src/app.rs`, narrow orchestration/command handling in `src/main.rs`, command/input/render coverage in `src/tui/{commands,input,render,render_tests}.rs`, this ExecPlan, and required harness documents.

Forbidden files/areas: oracle/reference trees; provider protocols; tool implementations/approval policy; Session V4 schema/storage; real user config/sessions/Keychain; installed binary; broad unrelated refactors.

Runtime state affected: process-only soft-pause request/boundary state. A pause may retain an unresolved assistant tool-call message or fully resolved tool outputs in memory until explicit `/unpause` or cancellation. Disposable tests/walkthroughs may mutate only temporary workspaces.

## Design and Invariants

### Typed boundary state

- Add process-only `SoftPauseState` with `Running`, `Requested`, `PausedBeforeTools`, and `PausedBeforeProvider` semantics (an equivalent enum with the same observable states is acceptable). It never enters Debug output, Session V4, requests, usage, hooks, or canonical messages.
- `/pause` during an active turn is idempotent and requests the next boundary. Idle use reports that no turn is active. `/unpause` resumes only a reached paused boundary; while merely requested it reports that the current operation is still finishing.
- Reaching a pause boundary keeps `TurnLifecycle` active and moves its visible phase to `Paused`. A plain response reaches a completed provider boundary before finalization, so a requested pause also holds checkpoint, goal/workflow, turn-completion, and queue-drain follow-on work until `/unpause`.

### Provider boundary

- After provider streaming and response hooks have completed, append the exact assistant response/tool-call message as today. If tools exist and pause is requested, retain that canonical unresolved tool-call message, start no tool/hook execution, and enter `PausedBeforeTools`.
- `/unpause` from that boundary invokes the existing `start_completed_tool_batch` path exactly once. Stale/duplicate unpause actions cannot start duplicate tool batches.
- If no tools exist, the same completed-provider gate retains the assistant response and pauses before finalization. `/unpause` invokes the existing plain-response finalizer exactly once.

### Tool boundary

- Let an already-running tool batch, ordered execution, approval, interaction, and its eligible hooks finish normally. Once every tool call is resolved and immediately before `start_llm_turn`, honor a requested pause by retaining the exact tool results and entering `PausedBeforeProvider`.
- `/unpause` from that boundary invokes the existing provider-continuation path exactly once. It must not rerun tools, hooks, approvals, interactions, or mutation receipt retention.

### Cancellation, queues, and owners

- Ctrl+C while requested or paused remains cancellation: clear soft-pause state, use the existing cancelled terminal outcome, preserve current interrupted-state semantics, and never resume implicitly.
- Turn activity remains true while paused, so ordinary composer input keeps existing queue/steer behavior and queue draining cannot occur. `/pause` and `/unpause` remain reachable through the Always command path.
- Global soft pause neither mutates nor substitutes for goal/workflow pause state. Resuming returns control to the same main-loop owner and its existing finite limits/evidence gates.
- No provider usage span or tool duration is fabricated across a pause. Completed provider usage remains completed; the next continuation begins its normal span only after unpause.

## Milestones

### M1 — Characterization and state machine

- Add failing focused tests for command visibility while busy, idle/requested/idempotent behavior, visible paused lifecycle, and cancellation clearing.
- Implement the process-only typed state and command request boundary.

### M2 — Provider and tool gates

- Gate both no-hook and response-hook provider-to-tool paths before `start_completed_tool_batch`.
- Gate every manual/direct tool-resolution convergence before `start_llm_turn`, then add exact-once unpause orchestration and stale-action tests.

### M3 — Visible behavior and compatibility

- Render concise paused status and dynamic command/help visibility at wide and narrow widths. Cover queued input, plain-answer completion, approval/interaction, goal/workflow ownership, and Ctrl+C distinction.

### M4 — Gates and released disposable walkthrough

- Run focused tests, `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smokes.
- With a temporary home/workspace and credentialless loopback provider, prove pause-before-tools prevents a file/tool side effect and provider continuation, unpause resumes once, pause-during-tool prevents the next provider request, cancellation remains terminal, and no real runtime state is touched. Stop fixtures and move the exact disposable root recoverably to Trash.

## Acceptance Criteria

- `/pause` and `/unpause` are discoverable and usable while a turn is busy. Idle, repeated, not-yet-reached, and already-paused uses are deterministic and non-destructive.
- A requested provider-boundary pause starts zero tools until one `/unpause`; a requested tool-boundary pause starts zero provider continuations until one `/unpause`.
- Canonical assistant tool calls and tool outputs are retained exactly once. Resume does not duplicate provider requests, tools, hooks, approvals, interactions, usage entries, or mutation receipts.
- Plain final responses finish normally without a pause request and are retained/finalized exactly once with one; cancellation stays distinct and cannot later resume; queued input remains queued while paused.
- Session V4 bytes/schema, goal/workflow durable pause semantics, headless/other runners, provider protocols, tool policy, and full repository gates remain unchanged.

## Validation

Focused: PASS (`cargo test activity`, `cargo test pause`, and six reviewed wide/narrow pause snapshots).

Completion: PASS (`cargo test --no-fail-fast`, 940 unit tests plus 28 CLI integration tests; `cargo fmt --check`; strict Clippy; release build; help/version/profile smokes).

Manual/visual: PASS. A released disposable TUI walkthrough proved pause-before-tools held a file write and provider continuation until `/unpause`, pause-after-provider held finalization, and Ctrl+C remained terminal cancellation. All loopback requests omitted Authorization, `--no-session` wrote no session, and the inspected fixture moved recoverably to `/Users/aji/.Trash/ca-tur078-walk.SWAxjP`.

Blocked checks: none currently.

## Progress

- [x] Existing lifecycle, provider finish, response-hook, tool-batch, and manual-resolution continuation seams traced.
- [x] Baseline focused activity/pause tests recorded.
- [x] Typed soft-pause state and commands implemented.
- [x] Provider/tool/finalization gates and exact-once unpause integrated.
- [x] Focused and repository-wide gates pass.
- [x] Released disposable walkthrough passes.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: pause only before tool execution and before provider continuation.
  Evidence: these are the two points where current code has completed and retained one side of the provider/tool protocol and is about to start new work.
  Consequence: no in-flight task is aborted, and `/unpause` can reuse existing exact continuation helpers.
- Decision: keep the active lifecycle and add a visible paused phase.
  Evidence: treating the turn as idle would allow new submission/queue drain against unresolved tool protocol state.
  Consequence: ordinary busy-state safeguards remain active while the user can still invoke Always commands.
- Decision: keep soft pause process-only and separate from goal/workflow state.
  Evidence: Session V4 cannot safely replay an unresolved runtime task boundary, while goals/workflows already own durable semantic pause controls.
  Consequence: process exit/restoration retains existing interrupted-session behavior instead of pretending it can resume execution automatically.

## Surprises and Discoveries

- Response hooks form part of the completed provider boundary; pausing before they finish would silently skip configured lifecycle behavior. The gate therefore belongs after a successful response-hook batch and immediately before tools.
- Plain provider completion also owns consequential follow-on work (checkpointing, goal/workflow progression, terminal outcome, and queue drain), so it uses a third `Finalization` continuation boundary rather than clearing a requested pause.

## Recovery and Rollback

Remove the two command registrations, process-only state, pause gates, and focused tests with local patches. Do not use destructive Git commands. Since no schema/config migration is introduced, rollback requires no runtime-state repair.

## Completion Record

Changed files: `src/activity.rs`, `src/app.rs`, `src/main.rs`, `src/tui/commands.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, six reviewed snapshots, this plan, and the required harness documents.

Commands and results: focused activity/pause/render tests, 940-unit plus 28-CLI full suite, formatting, strict Clippy, release build, profile/help/version smokes, and released disposable walkthrough all passed.

Remaining risks: pause remains deliberately process-only and boundary-based; it does not suspend an in-flight provider call, tool, approval, interaction, or hook.

Harness updates: architecture, continuity, testing, next steps, and parity reconciled; plan archived.
