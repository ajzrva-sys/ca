# MOD-019 — Model Capabilities, Reasoning, and Activity

Status: COMPLETE
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Make model capabilities explicit in configuration, transmit and render provider reasoning only when configured, and replace ambiguous busy/status state with an authoritative lifecycle whose completed outcome alone permits queue draining.

## Non-Goals

- Bundled or model-name-inferred capabilities.
- Cursor parity claims while `PAR-005` lacks a clean fixture.
- Workspace policy, skills, MCP, plans, agents, or review features owned by later tasks.
- Changes to oracle trees, runtime credentials, user sessions, installed binaries, or the live TUI process.

## Evidence and Current Behavior

- `src/config.rs::CaConfig` has legacy `model_context` only; provider model lists are strings.
- `src/agent/client.rs::ChatRequest` cannot express reasoning options, and `ChatBackend` has no request-options boundary.
- `src/agent/stream.rs::ProviderEvent` has text/tool/finish/usage/done but no reasoning delta.
- `src/app.rs::AppState` uses `queue::TurnState` plus free-form `status_message`; `src/queue.rs::should_drain_after` equates `Idle` with success.
- `src/main.rs::start_llm_turn` sets `"Thinking…"` before the request and uses the same state through provider wait, response, tools, and interactions.
- `Message::thinking` and `StreamingState::thinking` already preserve optional reasoning content, but no provider event populates them.
- `src/changes.rs::sanitize_terminal_text` handles a subset of terminal escapes and is not the central boundary for all provider-visible text.
- `PAR-005` is blocked, so this plan is a `ca` extension.

## Scope

Allowed files: `src/`, focused tests/snapshots, `Cargo.toml` only if required, this ExecPlan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, `ARCHITECTURE.md`.

Forbidden files/areas: `oracles/`, `reference/`, credentials, runtime config/session/history, installed `target/release/ca`, live processes.

Runtime state affected: none during automated validation; CLI smoke tests use local help/version exits only.

## Design and Invariants

- `[model_profiles."provider/model"]` is optional and takes precedence over legacy `model_context`; legacy string model lists remain valid.
- Unknown/unprofiled models send no reasoning request field and expose no effort control.
- `ReasoningEffort` is a closed serde enum; a profile declares its supported order and optional default.
- `ChatRequestOptions` selects a configured request-field format and reasoning stream field. Existing callers default to no options.
- Reasoning is a typed `ProviderEvent`; the shared assembler retains it, and headless emits it only under `--show-thinking`.
- `TurnLifecycle` owns phase, outcome, and phase timing. Only `TurnOutcome::Completed` permits queue draining; idle is not a success synonym.
- Lifecycle phases are authoritative: Sending, Waiting, Reasoning, Responding, Tool, Interaction, and Idle.
- Session V2 adds only an optional/defaulted reasoning-effort field, preserving old files and storage identity.
- Model changes choose the target profile default (or no effort) and visibly report the fallback.
- Terminal text is sanitized by one state machine covering CSI, OSC, DCS, APC, BEL, C0 controls, ST termination, and malformed/truncated escape sequences.

## Milestones

### M1 — Characterization and model-profile boundary

- Add failing tests for legacy/profile precedence, unknown-model behavior, effort defaults/cycling, and request JSON formats.
- Implement explicit profile/config types and `ChatRequestOptions`.
- Validation: focused config/client tests.

### M2 — Reasoning stream and persistence

- Add failing byte-split reasoning-field tests and assembler retention tests.
- Implement configured reasoning deltas across decoder, assembler, TUI, headless, checkpoint/subagent-compatible defaults, and session restore/save.
- Validation: focused stream/client/session/headless tests.

### M3 — Authoritative lifecycle and queue outcomes

- Add failing transition/drain tests.
- Introduce lifecycle types, migrate orchestration transitions, and gate draining on `Completed` only.
- Validation: queue and orchestration tests.

### M4 — Commands, key binding, rendering, and sanitization

- Add `/reasoning`, `/thinking`, Ctrl+T, collapsed reasoning, truthful phase/elapsed rendering, and wide/narrow snapshots.
- Centralize terminal text sanitization and add malformed-sequence coverage.
- Validation: input/command/render/sanitizer tests and reviewed snapshots.

### M5 — Completion gates and harness reconciliation

- Run focused tests, `cargo test`, `cargo fmt --check`, strict Clippy, release build, help/version smoke tests.
- Update plan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md`; move plan to completed only after all gates pass.

## Acceptance Criteria

- Legacy config parses unchanged; explicit profiles override legacy context metadata.
- Unknown models serialize no reasoning option and cannot cycle effort.
- Configured request formats and configured stream fields round-trip through tests.
- Reasoning persists separately from answer text and is collapsed unless explicitly expanded.
- Model changes visibly select the new profile default or disable reasoning.
- Lifecycle tests cover every phase and completed/failed/cancelled outcomes; only completed drains one queue item.
- Sanitization removes all specified control families without swallowing safe following text.
- Wide/narrow snapshots show stable reasoning/activity presentation.

## Validation

Focused: PASS — config/profile (3), client protocol (12), stream/assembler (7), lifecycle (1), queue (5), headless (10), commands (6), input (31), session (6), sanitizer (1), orchestration reasoning (1), and render (25) suites.

Completion: PASS — `cargo test` (209 unit + 15 CLI), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, `./target/release/ca --help`, and `./target/release/ca --version`.

Manual/visual: reviewed collapsed/expanded reasoning snapshots at 100×30 and 60×24 plus updated busy Sending-phase snapshots; 46 accepted snapshots and zero `.snap.new`. The live TUI was not touched.

Blocked checks: clean Cursor comparison remains blocked by `PAR-005`.

## Progress

- [x] Current configuration, request, stream, lifecycle, queue, command, input, session, and sanitizer ownership traced.
- [x] Characterization tests added.
- [x] Profile/request/stream implementation complete.
- [x] Lifecycle/queue implementation complete.
- [x] Commands/rendering/sanitization complete.
- [x] Completion gates and documentation complete.

## Decisions

- Decision: capability lookup uses exact `provider/model` keys only.
  Evidence: requirements forbid model-name inference and bundled profiles.
  Consequence: legacy `model_context` remains a compatibility fallback only for context limits.
- Decision: retain default no-options methods on shared provider boundaries.
  Evidence: checkpoint, test, headless, and subagent callers already share them.
  Consequence: only explicitly profiled main/headless requests gain reasoning fields.
- Decision: phase and outcome are separate.
  Evidence: an idle UI can follow completion, failure, or cancellation, while queue policy must distinguish them.
  Consequence: queue drain cannot infer success from non-busy state.

## Surprises and Discoveries

- `Message::thinking` and structured headless `show_thinking` already exist, so reasoning transport can reuse rather than redesign transcript/output storage.
- Idle cannot encode whether a turn completed, failed, or was cancelled; separating phase from outcome removed that ambiguity without changing queue identity.

## Recovery and Rollback

Use focused `apply_patch` reversals by milestone. Do not use destructive Git commands. Optional serde fields preserve compatibility if later milestones need to be backed out.

## Completion Record

Changed files: `src/activity.rs`, `src/terminal_text.rs`, `src/config.rs`, `src/agent/{client.rs,client_tests.rs,stream.rs}`, `src/{app.rs,changes.rs,headless.rs,main.rs,queue.rs,session.rs}`, `src/tui/{commands.rs,input.rs,input_tests.rs,render.rs,render_tests.rs,snapshots/*reasoning*.snap,snapshots/*streaming*.snap,snapshots/*queue_preview*.snap}`, and harness documentation.

Commands and results: focused suites PASS; `cargo test` PASS (209 + 15); format PASS; strict Clippy PASS; release PASS; local help/version PASS.

Remaining risks: no live provider reasoning dialect or interactive TUI was exercised; configured request/stream field names remain the operator's responsibility. Cursor parity remains unverified.

Harness updates: `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md` reconciled; this plan moved to completed.
