# WFL-070 — Declarative Workflow Engine

Status: COMPLETED
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Add an explicit, finite `/workflow run <path>` facility for validated TOML or JSON DAGs. A workflow runs ready steps in deterministic order through the existing main provider/tool/approval boundaries, captures bounded named outputs, enforces evidence gates, and persists only typed bounded execution state. Invalid definitions fail before provider traffic and no workflow file can execute embedded orchestration code or silently grant itself authority.

## Non-Goals

- JavaScript, shell-as-orchestration, arbitrary expressions, remote definitions, workflow marketplaces, schedules, parallel step execution, worktrees, multi-agent graph scheduling, best-of-N, or runtime plugin loading.
- Replacing `/goal`, existing plan/tasks, headless one-shot mode, or ordinary chat turns.
- Automatic workflow discovery or execution. `/workflow run` and a Cancel-first confirmation are required; bare mode refuses execution.
- Cursor parity. Oracle documentation informed the product priority only; no clean Cursor workflow fixture or Ante runtime observation establishes this behavior.

## Evidence and Current Behavior

- `src/work.rs` owns durable plan/tasks/evidence, but it is an interactive work artifact rather than an executable dependency graph. It has no definition loader, dependency validation, retry policy, named outputs, or scheduler.
- `SessionV4` accepts defaulted incremental fields and already persists bounded goal/work state (`src/session.rs`). A new defaulted workflow run can remain backward-readable without introducing Session V5.
- `start_llm_turn` and `finalize_completed_plain_response` own the only complete interactive provider/tool/approval loop (`src/main.rs`). Goal continuation proves that a request-only control prompt can reuse that loop without fabricating a canonical user message.
- `tool_selection::resolve` is the authoritative model-visible tool boundary, and `permission::evaluate` keeps Deny above process grants. A workflow step must only intersect the central tool set and add run-local grants after direct user confirmation; it cannot widen profile/model/capability/network authority.
- `RunCapabilities` already distinguishes bare, named, and default runs. Bare disables extension execution and persistent mutation, so it must reject workflow runs while allowing safe status/help.
- The supplied roadmap requires stable IDs, dependencies, prompts, model selection, grants, retry policy, outputs, evidence gates, acyclicity, at most 64 steps, zero retries by default, and at most three configured retries.

## Scope

Allowed files: a new workflow module; defaulted Session V4 workflow state; runtime capability, command/app/input/render state; main-loop routing; focused TOML/JSON/temp-session/loopback tests and snapshots; required harness documents.

Forbidden: oracle/reference trees, real providers/credentials/Keychain/user sessions/config, embedded interpreters, remote URL loading, implicit permission/network grants, background schedules, concurrent agents/worktrees, installed-binary mutation, or automatic execution.

Runtime state affected: only an explicitly confirmed current workflow run, its bounded Session V4 state when session persistence is enabled, ordinary canonical assistant/tool messages produced by its steps, and process-only exact run grants. Definitions are read-only workspace inputs and are never rewritten.

## Design and Invariants

### Definition and loading

- `WorkflowDefinitionV1` is `#[serde(deny_unknown_fields)]`, versioned, and parsed by extension as TOML or JSON. The loader accepts one regular UTF-8 workspace file, rejects symlinks and files over 256 KiB, and performs no network, process, MCP, provider, or config mutation.
- Definition and step IDs use lowercase ASCII `[a-z][a-z0-9_-]{0,63}`. Definitions contain 1–64 ordered steps; prompts are nonempty and at most 16 KiB each. Duplicate IDs/output names, unknown/self dependencies, unsupported versions/extensions, and unresolved output references fail atomically.
- A deterministic Kahn pass rejects cycles and records definition order as the tie-breaker. Runtime scheduling is sequential in this slice: select the first pending step whose dependencies succeeded. A failed dependency blocks the run rather than silently skipping descendants.
- Each step may specify an exact configured `provider/model`; omission freezes the current pair when the run is confirmed. All exact routes and requested built-in/active-MCP tool names validate before confirmation and before provider traffic.

### Tools, grants, retries, and evidence

- Omitted `tools` inherits the centrally resolved Main set; an explicit list is a restrictive intersection and `[]` means no tools. Selection never activates MCP, bypasses model capability, or grants execution.
- Optional grants are typed exact-tool, workspace-relative path-glob, or parsed simple-command-prefix requests. Validation rejects traversal, external roots, unsafe shell syntax, unknown/unadvertised tools, resource-wide grants for path/process/network tools, and all network grants. The confirmation preview shows every safe grant label. Confirmed grants live only on `WorkflowRun`, are supplied only for its active step, remain below persistent Deny/Ask rules, and disappear when the run ends or is cleared.
- A step's retry policy defaults to zero and accepts at most three retries. Causes are closed to `provider_error` and `evidence_failed`. Automatic provider retry is allowed only when the attempt produced no usable provider output and started no tool call; evidence retry is allowed only when no mutating tool succeeded. Other failures stop the step with explicit status. Existing low-level transport retry and explicit `/retry` behavior remain unchanged outside workflows.
- Evidence gates are local typed predicates: nonempty assistant output (the default) and successful execution of an exact named tool. Each attempt records only safe tool names/statuses, never arguments/results. Every configured gate must pass before success; a failed gate follows the explicit retry policy or fails the step.

### Outputs and execution state

- A successful step may name its output. The final assistant text is UTF-8 bounded to 32 KiB and total retained workflow outputs are capped at 256 KiB. Dependency outputs are automatically included in later step prompts inside explicit untrusted-data delimiters; definitions cannot interpolate arbitrary files, environment variables, secrets, or non-dependency outputs.
- `WorkflowRun` has a fresh UUID, source definition identity/digest, frozen default route, typed run/step statuses, attempt counters, safe tool evidence, bounded outputs, current step, timestamps, and a generation. It stores neither raw definition bytes nor credentials/endpoints/tool arguments/results.
- Session V4 gains one defaulted optional workflow run. Old sessions load with none; a restored running run becomes paused and requires `/workflow resume`; historical branches omit it and full forks receive a fresh run identity/source link. No active goal and workflow may own the main loop simultaneously.
- A step begins with a request-only instruction, so no synthetic user message is persisted. Ordinary assistant/tool messages remain canonical for transparency. The step route and restrictive tool set are pinned through that step's complete tool loop, then cleared before scheduling the next step or returning to normal input.
- At most one workflow step is active. Limits are finite by construction: 64 steps, four attempts per step, existing per-turn tool-loop limits, and a hard 256 provider-attempt ceiling. Queued/user input, interrupt, approval cancellation, session load/branch/rewind, hook failure, or stale generation pauses the run; it never resumes without `/workflow resume`.

### Commands and visible state

- Add visible `Always` `/workflow` with `run <path>`, `status`, `pause`, `resume`, and `clear`. Run is accepted only while idle; status/pause remain available during work. Mutating commands lock while a step/evaluator/tool action is active.
- Loading a valid definition opens a Cancel-first confirmation showing file label, step count/order, exact model overrides, retry totals, tool restrictions, and requested grant labels. Confirmation never starts a provider request until Run is explicitly selected.
- `/workflow status` shows the bounded DAG/step states, active route, attempts, gate results, outputs by safe name/size, and actionable pause/failure text. Title/status surfaces identify the active step. Wide/narrow snapshots cover preview, running, waiting approval, paused, failed, blocked, and complete states.
- `/workflow clear` removes only terminal or paused run state after confirmation and never deletes the definition or canonical history. Bare reports that workflow execution is disabled and points to a normal/default profile.

## Milestones

### M1 — Definition, validator, and durable state

- Add failing TOML/JSON loader tests for bounds, unknown fields, IDs, duplicates, dependencies, cycles, routes, tools, grants, retries, outputs, gates, symlinks, traversal, and zero-I/O rejection.
- Add typed scheduler/run tests for deterministic readiness, dependency failure, attempt limits, output bounds, restore/fork behavior, and run/grant redaction.

### M2 — Command, preview, and central authority

- Add `/workflow` parsing/discovery/busy tests, Cancel-first preview state, bare refusal, exact route/tool validation, run-local grant intersection, and wide/narrow snapshots.
- Prove requested tools cannot widen central selection, persistent Deny wins, network remains separately prompted, and invalid definitions make zero provider requests.

### M3 — Main-loop scheduler and persistence

- Route request-only step prompts through the existing main loop with frozen per-step route/tools/grants, generation-safe completion, local evidence evaluation, bounded output capture, safe retry decisions, deterministic next-step scheduling, and explicit pause/clear behavior.
- Add loopback tests for linear/branching DAGs, model overrides, no-tools steps, tool continuations/approvals, retry/no-retry cases, output propagation, failed dependencies, queue/user interruption, save/resume, and goal/workflow mutual exclusion.

### M4 — Rendering, walkthrough, and gates

- Review all wide/narrow workflow snapshots and ensure no definition prompt/output/tool argument/credential enters unsafe manager/debug/snapshot surfaces.
- Run focused/full/format/snapshot/strict-Clippy/release/smoke gates and a released-TUI temporary-home walkthrough over a branching fixture, one approved scoped grant, one output dependency, and one evidence gate.

## Acceptance Criteria

- Valid TOML and JSON definitions with at most 64 steps schedule every ready step once in deterministic definition order; cycles, unknown dependencies, duplicate IDs/outputs, and a 65th step fail before provider traffic.
- Retry count defaults to zero and values above three fail. Only configured closed causes retry, and an attempt with usable output, a started tool call, or a successful mutating tool is never replayed automatically.
- Step tool exposure is an intersection of the central Main set; grants are explicit, previewed, run-local, Deny-dominated, non-network, and scoped. A definition alone cannot authorize a side effect.
- Exact per-step provider/model overrides remain pinned through tool continuations and never change current/default configuration. Missing routes fail before the first request.
- Named outputs and gate evidence are bounded, dependency-scoped, secret-free in state/debug/rendering, and durable only with the current session. Downstream prompts receive only successful direct/transitive dependency outputs in explicit untrusted blocks.
- Restored running state is paused; stale results, user input, interrupt, session replacement, hook failure, and cancellation cannot advance the DAG. `/workflow resume` is explicit.
- Existing chat, `/goal`, `/retry`, headless, scripted, review, checkpoint, subagent, provider setup, approvals, profiles, and Session V1–V4 behavior remain unchanged when no workflow runs.

## Validation

Focused: `cargo test workflow --no-fail-fast` passed 24 matching tests. This covers bounded/strict TOML and JSON parsing, name/dependency/cycle/route/tool/grant/retry/evidence validation, deterministic scheduling, bounded outputs, safe restoration/forks, central authority, command/input behavior, request isolation, and a two-step loopback execution chain.

Completion: `cargo test --no-fail-fast` passed 834 unit tests and 28 CLI integration tests. `cargo fmt --check`, zero `.snap.new` hygiene, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` all passed. Release smoke passed for version, help, the bare-profile model catalog, and invalid-profile rejection under a disposable home.

Manual/visual: reviewed 16 deterministic wide/narrow snapshots for preview, clear, running, waiting approval, paused, failed, blocked, and complete. The released binary completed an isolated three-step branching workflow through four credentialless loopback requests: the first step used one explicitly previewed `read_file` workspace-path grant and successful-tool evidence gate, its bounded output reached both direct dependents, both dependent requests advertised zero tools, and `/workflow status` showed three succeeded steps. The disposable Session V4 contained a complete run and zero canonical user messages.

Blocked checks: none.

## Progress

- [x] Current session/work/goal/main-loop/tool-policy seams characterized and v1 safety decisions recorded.
- [x] Definition loader, validator, scheduler, and Session V4 compatibility complete.
- [x] Command/preview/render and run-local authority boundaries complete.
- [x] Main-loop execution, retry/evidence/output/pause orchestration complete.
- [x] Repository-wide gates and isolated released-TUI walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: sequentially execute ready DAG nodes in definition order.
  Evidence: the roadmap requires dependencies and acyclicity but reserves dependency-aware agent concurrency for AGT-072.
  Consequence: WFL-070 proves deterministic workflow semantics without introducing parallel provider/tool ownership.
- Decision: reuse the canonical main tool loop with request-only step prompts.
  Evidence: it is the only path already enforcing prepared requests, central tool selection, approvals, sandbox, hooks, fallback, usage, cancellation, and tool-call continuation together.
  Consequence: assistant/tool results remain visible and canonical, while workflow control text is never attributed to the user.
- Decision: grants are previewed run-local permission matchers, not persistent policy and not tool selection.
  Evidence: workflow files are data and cannot grant themselves authority; the permission evaluator already provides Deny-over-grant precedence.
  Consequence: clearing/completing the run drops grants, and network always retains its distinct prompt.
- Decision: named outputs are automatic dependency context rather than a template language.
  Evidence: arbitrary interpolation adds expression/injection/path authority and is unnecessary for DAG data flow.
  Consequence: downstream steps receive bounded successful dependency outputs under explicit untrusted-data labels only.
- Decision: keep Session V4 with one defaulted field.
  Evidence: incremental V4 fields are an established compatibility boundary and lazy history is deferred to SES-076.
  Consequence: existing files remain readable without eager migration.

## Surprises and Discoveries

- `WorkState` events are currently recorded for direct shell commands and fresh reviews, not every model tool execution. Workflow evidence therefore needs its own bounded name/status observations instead of assuming the general work ledger is complete.
- `ResolvedToolResult` carries a call ID but not the tool name. The active assistant tool-call list is the safe correlation source; workflow state must never retain arguments or model-visible results.
- Goal continuation already establishes the request-only main-loop pattern, but workflow ownership additionally needs exact route, tool-intersection, and run-local-grant pinning across tool continuations.
- The first repository-wide run found only an intentional `/usage` snapshot change from the new `WorkflowStep` category; after review and snapshot acceptance, all 834 unit tests passed.
- Strict Clippy found two slice-vs-`Vec` signatures, a needless question mark, and default-field reassignment. Fixing those local issues required no behavior change and the strict lint gate then passed.
- The released walkthrough initially appeared inert because a new temporary workspace correctly opened the trust gate. After explicit trust acceptance, the Cancel-first preview, scoped grant, tool continuation, branch scheduling, and completion status all executed as designed.

## Recovery and Rollback

Remove the command/scheduler/runtime fields and leave the defaulted Session V4 workflow field ignored. A paused workflow can always be cleared without deleting canonical messages, definitions, work evidence, or sessions. All tests and walkthroughs use disposable roots and loopback providers.

## Completion Record

Changed files: `src/workflow.rs`; workflow state/orchestration integration in `src/{app,main,request,runtime,session,session_portability,usage}.rs`; command/input/render coverage in `src/tui/{commands,input,render,render_tests}.rs`; 16 workflow snapshots plus four intentionally updated usage snapshots; and the required harness documents.

Commands and results: focused workflow suite 24 passed; full suite 834 unit plus 28 CLI passed; formatting, snapshot hygiene, strict Clippy, release build, version/help/profile smoke, and the released-TUI loopback walkthrough passed.

Remaining risks: execution is deliberately sequential, not concurrent; definitions are local workspace inputs and remain external dependencies at resume time; evidence is limited to nonempty assistant output or exact successful tool names; and run-local command/path grants still require the same central policy/sandbox limits as ordinary calls. Parallel agent/worktree scheduling belongs to later plans.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now describe the implemented boundary, validation evidence, intentional-extension status, and `WT-071` handoff.
