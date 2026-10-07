# HED-017 — Structured Headless Automation

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-09

## Goal

Provide deterministic text, aggregate JSON, and streaming JSONL headless execution with stable `ca.headless.v1` events, fail-closed approvals, explicit exit codes, and an injectable runner.

## Non-Goals

- TUI interaction redesign (`INT-018`), reasoning-provider fields (`MOD-019`), or workspace sandbox enforcement (`WSP-020`).
- Claiming Cursor parity; this remains a documented `ca extension` because `PAR-005` has no clean fixture.
- Network authorization beyond carrying the explicit `--allow-network` boundary for `WSP-020`.

## Evidence and Current Behavior

- `src/main.rs::run_headless` owns config loading, provider setup, stream assembly, text output, tool execution, and loop termination in one function.
- Empty input prints usage and exits 0; provider/setup/protocol/max-turn errors collapse into the binary's generic error path.
- Headless tool calls execute without consulting approval rules.
- Output is assistant text on stdout and ad-hoc tool diagnostics on stderr; no stable machine-readable lifecycle exists.
- The shared SSE assembler and typed tool registry completed under `SSE-008` and `TOL-016` are available as runner boundaries.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, new `src/headless.rs`, narrow provider/tool interfaces, `tests/cli.rs`, hermetic fixtures, and harness docs.

Forbidden: oracle/reference trees, credentials, user runtime state, installed binary, live TUI, sandbox/network-policy implementation.

Runtime state affected: subprocess tests use temporary homes/config/output paths and loopback-only fake providers.

## Design and Invariants

- CLI parsing remains side-effect free; new flags are parsed before config/provider initialization.
- `HeadlessRunner` accepts a `ChatBackend`, explicit config/input/cwd, output writers, and tool executor boundary.
- Every structured line is one `HeadlessEnvelopeV1` with schema, sequence, run ID, and typed event; aggregate JSON contains the same ordered events.
- Structured stdout contains only JSON. Diagnostics and text-mode tool summaries use stderr.
- Default and auto-review modes fail closed when authorization is absent; persistent denial becomes a typed tool result, persistent allow executes, and `--yolo` bypasses prompts only.
- Provider transport/HTTP map to exit 5; framing/protocol/incomplete response to 6; max turns to 7; internal invariants to 8; approval/interaction to 4; empty input to 2.
- A downstream broken pipe stops the runner and returns 0 after cleanup.

## Milestones

### M1 — CLI and event characterization
- Add failing parse/subprocess tests for formats, aliases, conflicts, empty input, and local help.
- Define stable serialized envelopes/events and golden JSON/JSONL expectations.

### M2 — Injectable runner
- Extract provider/tool loop behind explicit input/options/writers.
- Emit text, JSON, and JSONL deterministically; write the final message when requested.

### M3 — Approval and exit semantics
- Apply persistent rules and fail-closed modes before execution.
- Map setup/provider/protocol/max-turn/internal/broken-pipe outcomes to documented codes.

### M4 — Hermetic integration and completion
- Add loopback fake-provider subprocess coverage for stdout/stderr, lifecycle order, failures, approvals, and output-last-message.
- Run focused/full/format/Clippy/release/help/version gates and reconcile harness docs.

## Acceptance Criteria

- Default text remains human-readable; JSON is one valid aggregate document; JSONL is independently parseable line-by-line.
- Every structured event has `schema`, monotonic `sequence`, and stable per-run `run_id`.
- Empty input exits 2; setup 3; approval 4; transport 5; protocol 6; max turns 7; invariant 8.
- Structured stdout contains no diagnostics; tool start/finish and usage are represented as typed events.
- Unapproved tools never execute; persistent rules and yolo behavior are covered.
- Broken pipe returns 0 and does not leave owned jobs running.

## Validation

Focused: PASS — 9 injectable runner tests and 15 subprocess CLI tests, including loopback provider/tool rounds and exit codes.

Completion: PASS — 189 unit tests plus 15 CLI integration tests; format, strict Clippy, release, help, and version all pass.

Manual/visual: CLI output review only; no live TUI.

Blocked checks: clean Cursor parity remains blocked by `PAR-005` and is not claimed.

## Progress

- [x] Traced current CLI/headless/provider/tool/approval boundaries.
- [x] Added failing CLI and output-contract tests.
- [x] Extracted runner and implemented formats/events.
- [x] Enforced approvals and exit mapping.
- [x] Completed validation and harness updates.

## Decisions

- Decision: keep the stable schema in a dedicated headless module rather than serializing TUI state.
  Evidence: lifecycle automation requires transport-neutral events and exact stdout control.
  Consequence: later interaction/reasoning additions extend the versioned event enum without coupling to ratatui.

## Surprises and Discoveries

- Persistent deny decisions must be evaluated before interaction/network gates so an explicit deny remains authoritative without being mislabeled as an unresolved prompt.
- Aggregate JSON cannot observe a broken stdout pipe until its one terminal write; JSONL/text cancel immediately on the first failed write.

## Recovery and Rollback

Inverse-patch only listed files; temporary subprocess homes and loopback servers are self-cleaning.

## Completion Record

Changed files: new `src/headless.rs`, `src/main.rs`, `tests/cli.rs`, and harness docs.

Commands and results: focused suites PASS (9 + 15); `cargo test` PASS (189 + 15); `cargo fmt --check`, strict Clippy, release build, help, and version PASS.

Remaining risks: reasoning events are schema-reserved but cannot be emitted until provider reasoning deltas land in `MOD-019`; sandbox-enforced network/process boundaries remain `WSP-020`.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record the new runner and evidence.
