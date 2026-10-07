# HOK-052 — Complete Hook Lifecycle

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Make configured lifecycle hooks asynchronous, bounded, cancellable, process-group-cleaned, sandboxed with network denied by default, and observable/repairable through a safe `/hooks` manager while firing the supported main-session lifecycle events exactly once at their documented boundaries.

## Non-Goals

- Project/plugin hook discovery, hook installation, command editing in the TUI, network-enabled hooks, hook marketplaces, or runtime code plugins.
- Applying user hooks to provider probes, `/btw`, checkpoints, fresh reviews, subagents, headless/scripted runs, MCP-server internals, or user-issued `/shell` jobs in this slice. HOK-052 owns the interactive main-session hook contract only; later runner unification must opt other contexts in explicitly.
- Arbitrary matcher expressions, environment inheritance controls beyond the existing allowlisted hook environment, persistence of hook results, or Cursor parity.
- Changing hook command strings, copying oracle implementation code, or modifying any oracle/reference file.

## Evidence and Current Behavior

- `src/hooks.rs::run_hooks` runs `sh -c` synchronously, polls with `std::thread::sleep`, buffers complete stdout/stderr, kills only the immediate child on timeout, and can block the TUI loop.
- `src/main.rs` invokes `SessionStart` synchronously after the first frame and ignores denial; `BeforeSubmitPrompt` runs only after the user message is appended; `PreToolUse` runs on one persistent-rule path but not ordinary/approved/ordered paths. Post, failure, thought, and response events are never fired.
- `HookConfig` stores event/command/timeout/fail-closed only. There is no enable flag, safe management snapshot, config mutation, command, TUI state, or failure-repair surface.
- Main-session model tools reach `execute_tool_dispatch` through immediate, persistent-rule, approval, and ordered-continuation paths, so wrapping that dispatch boundary is the narrowest way to guarantee one Pre and one terminal Post/PostFailure for actual executions.
- `src/sandbox.rs::SandboxBackend::prepare_shell` and the existing job/MCP runners provide the local process-isolation and process-group patterns. Hook commands are already shell strings, but must never enter manager state, snapshots, or debug output.
- The Codex oracle documents lifecycle hooks and a startup hook review UI (`oracles/codex-main/docs/config.md` and `oracles/codex-main/codex-rs/tui/src/startup_hooks_review.rs`); ECC documents duplicate-hook execution as a real failure mode. These are architecture/product lessons only, not Cursor or runtime parity evidence.

## Scope

Allowed files:

- `src/hooks.rs`, `src/{config,app,main}.rs`
- `src/tui/{commands,input,render,render_tests}.rs`, focused tests, and deterministic snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`
- This ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, real user config/sessions/Keychain, installed binary, external services, and unapproved dependency additions
- Provider protocol, Session V4, hook command text editing, remote hooks, plugins, scheduled hooks, and lifecycle expansion beyond the approved seven events

Runtime state affected:

- Hook subprocesses explicitly configured in the selected ordinary profile.
- The selected profile's hook `enabled` scalar only after an explicit TUI toggle. Tests/walkthrough use fake executables and disposable homes/workspaces.

## Design and Invariants

- Add default-true `enabled` to `HookConfig`; existing files remain enabled without rewrite. Validate every event, positive timeout, and bounded timeout before runtime activation. `ConfigStore::update_hook_enabled(index, expected_event, enabled)` performs a comment-preserving atomic exact-index patch and fails if the raw entry changed. Bare cannot write.
- Replace the synchronous API with an injected process-scoped `HookRunner`. It owns the command-bearing configs internally; public management snapshots expose only index, event, enabled, timeout, fail mode, network policy, and typed last result. Commands, stdout/stderr, payloads, resolved environment values, and lengths never enter `AppState`, actions, events, snapshots, or debug output.
- One invocation serially runs matching enabled hooks in config order. Payload JSON, stdout, stderr, additional context, denial text, hook count, and timeouts are bounded. Non-JSON empty/legacy output remains success; malformed or oversized structured output is a typed safe failure. External text is terminal-sanitized before any user/model-visible use.
- Spawn through `SandboxBackend::prepare_shell` with the current effective sandbox mode, exact workspace roots/temp directory, and `network_allowed = false`. Every child enters its own process group. A drop guard kills the group on timeout, cancellation, task abort, panic, or shutdown and removes its dedicated temporary directory. On modes that cannot OS-enforce network denial, the safe result and `/hooks` status state that network is policy-only rather than claiming isolation.
- Hook results are typed as Success, Denied, Failed, TimedOut, or Cancelled, with fail-open/closed behavior applied separately. Safe per-hook results are process-only and keyed by exact index/event; reloading changed hook structure invalidates mismatched results.
- `SessionStart` begins on the hook worker after the first frame. Until it completes, prompt submission is locked but the TUI remains responsive. A denied/failing/timed-out fail-closed hook opens `/hooks` in a blocked state. Quit, safe inspection, toggling, and explicit test remain available; normal submission remains blocked until all enabled fail-closed SessionStart hooks pass or are disabled and revalidated.
- `BeforeSubmitPrompt` runs before canonical user-message append and provider start. While it runs the draft is held process-locally; denial/fail-closed failure restores the editable original draft and sends no provider request. Successful additional context is appended only after completion, then the canonical message is appended once.
- Actual main-session tool dispatch is wrapped once, after approval/policy resolution and before execution. It fires PreToolUse once; a denial prevents dispatch and becomes the tool result. One PostToolUse fires after success, otherwise one PostToolUseFailure fires after failure/cancellation/denial returned by dispatch. Interactions, approval waits, pre-resolved policy denials, and invalid/unregistered calls are not executions and do not synthesize hook pairs.
- A completed provider turn fires AfterAgentThought once when nonempty reasoning completed, then AfterAgentResponse once for the completed response. Tool-call responses fire before tool execution; plain responses fire before final turn completion. Fail-closed lifecycle failures block the next continuation/finalization and open the repair manager rather than silently losing canonical state.
- Add visible `/hooks [filter]`, idle-safe even when startup is hook-blocked. The searchable list shows safe status and selected-hook detail. `Space` toggles, `Shift+T` explicitly tests, `Shift+R` reloads config/snapshots, and `Esc` closes only when not startup-blocked. Commands are edited only in the displayed config file path. Busy actions lock controls; Esc cancels a test and generation IDs discard stale results.
- `/config` routes a Hooks row to the same manager. Bare shows hooks disabled/view-only. Empty, malformed-config, blocked-startup, busy, cancellation, save/test failure, and narrow-terminal states remain usable.

## Milestones

### M1 — Characterize lifecycle gaps and safe config

- Add failing tests for default-enabled compatibility, invalid event/timeout rejection, exact toggle patches/comment preservation/bare refusal, missing Post/Failure/Thought/Response calls, BeforeSubmit ordering, and duplicate Pre execution.
- Add fake hook fixtures that record only event names/IDs in temporary files; do not use real config or external network.
- Validation: focused tests fail only on missing HOK-052 behavior.

### M2 — Build the bounded cancellable runner

- Implement typed safe snapshots/outcomes, bounded JSON I/O, sandbox preparation, process-group drop cleanup, timeout/cancellation, serial matching execution, and internal last-result storage.
- Add fake executable tests for success/deny/fail-open/fail-closed/timeout/oversize/malformed output, environment/payload caps, network denial, child cleanup, and command/output non-leakage.
- Validation: hook-runner/config suites pass under temporary roots.

### M3 — Centralize main-session lifecycle orchestration

- Make SessionStart and BeforeSubmit asynchronous with repairable blocking and correct canonical ordering.
- Wrap every actual main-session tool-dispatch route exactly once and add thought/response completion hooks.
- Add orchestration tests for ordinary, allowed-rule, approved, ordered, denied, failed, cancelled, and stale-generation paths.
- Validation: lifecycle counters prove exact once and event-loop responsiveness.

### M4 — Add `/hooks` manager and repair flow

- Add command/config routing, safe snapshots, filter/navigation/toggle/reload/test/cancel, busy locking, startup-blocked restrictions, and inline feedback.
- Add reviewed wide/narrow snapshots for populated, empty, no-match, busy, last-result classes, blocked startup, bare, and inline failure.
- Validation: command/input/orchestration/render suites pass with zero pending snapshots.

### M5 — Complete gates and isolated walkthrough

- Run focused suites, full tests, formatting, strict Clippy, release build, and help/version/profile smoke.
- In a disposable home/workspace, use fake hook scripts to demonstrate responsive SessionStart, BeforeSubmit ordering, exact Pre/Post/Failure counts, timeout descendant cleanup, startup fail-closed repair by disabling the hook, reload, explicit test, and bare isolation. Inspect only temporary event logs and config.
- Reconcile the five harness documents, archive this plan, and activate REQ-053 only after every gate passes.

## Acceptance Criteria

- No hook command executes synchronously in the TUI reducer/event loop; a slow hook leaves redraw/cancel/manager controls responsive.
- Timeout, cancellation, task abort, and shutdown reap the exact hook process group and temp directory. Output/payload/context bounds cannot allocate or retain unbounded data.
- Hook commands, stdout/stderr, request payloads, and sensitive lengths never enter management state, debug output, snapshots, or sessions.
- Existing hook config stays enabled without rewrite; invalid events/timeouts fail before startup activation; exact toggles preserve comments/unrelated TOML and bare never writes.
- SessionStart fail-closed failures produce a repairable blocked TUI. BeforeSubmit completes before canonical append/provider I/O and restores denied drafts.
- Every actual main-session tool execution receives exactly one Pre and exactly one success/failure terminal hook; approval/policy/ordered routes do not duplicate or omit lifecycle calls.
- Thought and response events fire once at completed provider boundaries, never for stale/partial/cancelled generations.
- `/hooks` can safely inspect, filter, toggle, reload, test, cancel, and recover without exposing command text. All visible states have reviewed wide/narrow snapshots.
- Focused/full gates and the isolated walkthrough pass without real state, credentials, oracle changes, or installed-binary mutation.

## Validation

Focused:

- `cargo test hooks::tests --no-fail-fast`: PASS, 38 bounded-runner, validation, cleanup, lifecycle, manager, and non-leakage tests.
- `cargo test hook_manager --no-fail-fast`: PASS, 6 input/orchestration/render tests after normalizing real crossterm Shift-letter events.
- Hook manager snapshots: PASS, 16 reviewed wide/narrow populated, empty, no-match, busy, failure, cancellation, bare, and startup-blocked states; zero pending snapshots.

Completion:

- `cargo test --no-fail-fast`: PASS, 606 unit tests plus 25 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/bare-profile smoke: PASS (`ca 0.1.0`); bare model listing used only the disposable home and created no config.

Manual/visual:

- PASS in a disposable home/workspace with a credentialless loopback provider and temporary hook commands. A delayed SessionStart rendered a responsive first frame; `/hooks` loaded all safe rows; explicit Test ran from the actual terminal; successful and failed tool calls produced exact Pre plus matching Post/PostFailure order.
- PASS for fail-closed startup repair: the failed SessionStart opened `/hooks`, disabling that exact entry revalidated startup and unblocked submission, while bare mode showed every entry read-only and refused Test without adding an event-log line.
- PASS for timeout cleanup: an explicit test timed out at 1005 ms and its spawned 30-second child PID was absent immediately afterward. The loopback server and TUI exited cleanly.

Blocked checks:

- None currently.

## Progress

- [x] Traced synchronous runner, partial call sites, config schema, sandbox/process-group seams, and current tests.
- [x] Created the active ExecPlan before implementation.
- [x] Added failing characterization/config/runner tests.
- [x] Implemented bounded cancellable hook execution and safe snapshots.
- [x] Centralized exact main-session lifecycle orchestration.
- [x] Implemented and reviewed the `/hooks` repair manager.
- [x] Passed focused and repository-wide gates.
- [x] Completed isolated walkthrough and harness reconciliation; archival is the final mechanical step.

## Decisions

- Decision: only actual interactive main-session dispatches are hook-eligible in HOK-052.
  Evidence: current hook configuration is constructed in the TUI path, while review/checkpoint/subagent/headless have deliberately isolated runner contracts.
  Consequence: this plan completes one truthful boundary without silently injecting user executables into isolated contexts; later prepared-runner work must opt in explicitly.
- Decision: hook commands remain shell strings but are entirely absent from manager state.
  Evidence: backward compatibility requires the existing config format, and the approved plan says commands remain config-file-edited and never enter manager snapshots.
  Consequence: the TUI can enable/test exact configured entries by index/event while showing only safe metadata.
- Decision: batch order is config order and no matching hooks run concurrently.
  Evidence: deterministic deny/context composition and exact-once auditing are more important than hook latency; each command already has a bounded timeout.
  Consequence: one event has simple cancellation and failure semantics, while unrelated provider/tool work stays asynchronous.

## Surprises and Discoveries

- The current `BeforeSubmitPrompt` name is misleading: it runs after canonical append and ignores denial. HOK-052 must move the event boundary, not merely replace the blocking function.
- PreToolUse currently fires only for the `AllowedByRule` fast path; the ordinary no-approval path bypasses it, while approved/ordered paths call dispatch elsewhere. Exact-once behavior therefore requires a central dispatch wrapper.
- Existing timeout cleanup calls `Child::kill`, which does not guarantee descendant cleanup. The process-group drop guard is required even if the future is aborted before normal result handling.
- Real crossterm Shift-letter input arrives as an uppercase `Char` rather than a lowercase character plus modifiers. The first live Test attempt exposed this; `/hooks` now accepts both forms for Shift+E/Shift+R/Shift+T and has a regression test using the real uppercase event.

## Recovery and Rollback

- Code rollback is patch-only and limited to HOK-052-owned files. No destructive Git operation is permitted.
- A failed toggle leaves the original raw config unchanged. A failed hook changes only process-local safe status/context; fail-open/closed determines continuation.
- Tests and walkthroughs own explicit temporary roots and fake executables. Move only the recorded root to Trash after inspection; never clean user hook processes or state by broad pattern.

## Completion Record

Changed files: `src/hooks.rs`, `src/config.rs`, `src/app.rs`, `src/main.rs`, `src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`, hook-manager snapshots, this plan, and the five harness documents.

Commands and results: focused hook/config/input/orchestration/render suites passed; final completion gates passed with 606 unit and 25 CLI tests, formatting, strict Clippy, release build, local help/version/bare-profile smoke, and zero pending snapshots.

Remaining risks: HOK-052 intentionally applies configured hooks only to the interactive main session. Other runners remain hook-free unless a later shared-runner design opts them in explicitly. Policy-only sandbox modes can report network denial policy but cannot claim OS enforcement.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the completed boundary and route the sequential program to REQ-053.
