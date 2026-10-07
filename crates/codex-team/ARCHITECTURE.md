# ARCHITECTURE

## Protected evidence hashing

`src/verification/protected_tree.rs` hashes ignored `oracles/` and `reference/`
trees using a committed HEAD-only manifest. Algorithm
`sha256-merkle-v2-ds-store-excluded` removes exact `.DS_Store` components from
the walk before counts and hashes because Finder rewrites them independently of
evidence. Similar names remain protected. Traversal is still no-follow,
single-link/type bounded, double-scanned for preview, and revision/digest bound
for apply; worktree-only manifest bytes never grant trust.

## Purpose

`codex-team` is a local Rust CLI that coordinates provider-backed roles around Markdown control docs, Rust-owned queue state, deterministic verification, guard checks, evidence, telemetry, and commits. Codex remains the default role engine; Ollama, MLX LM, and the pilot-only restricted `ca role-run` engine are opt-in.

## Entry Points

- `src/main.rs` calls `codex_team::cli::run()`.
- `src/cli.rs` owns Clap parsing for `init`, `commit-pending`, `task-import`, `verify`, `step`, `run`, `status`, `cost`, `route-bench`, `doctor`, `providers`, `commands`, `watch`, `watch-loop`, `observe`, `learn`, `mission`, `models`, `disposition`, and `mcp`.
- Machine-readable receipts include `step`/`status`/`disposition`/`doctor`, task import, pending commit, and built-in verification JSON; `status --observe` remains the read-mostly CLI snapshot (see `docs/HERMES.md`). MCP uses structured SDK tool results rather than CLI rendering.
- `src/app.rs` is still the conductor for most orchestration behavior.

## Subsystems

| Area | Primary files | Responsibility |
|---|---|---|
| Conductor | `src/app.rs`, `src/app/support.rs` | Cycle routing, state transitions, role prompts, deterministic lanes, support packets, accept/commit |
| Queue | `src/queue/*` | `NEXT_STEPS` parsing, dependency gates, non-AI queue handling, tracking-doc patches |
| Verification | `src/verification/*`, `src/app.rs` | Typed recipes, ValidatedCommand execution, plan/evidence DTOs in verification; plan construction/persistence orchestration still partly in `app` |
| Guard | `src/guard/*` | Repository snapshots, allowed-file globs, changed-file reports, diff facts |
| Codex adapter | `src/codex/*`, `src/app.rs` | `run.rs` owns `codex exec` process IO; `stream.rs` owns pure JSONL helpers; live rendering still mostly in `app` |
| Restricted `ca` adapter | `src/ca/run.rs`, `src/config.rs`, provider-neutral dispatch in `src/app.rs` | Builds the versioned role request, fixed tools and process grants; supervises one `ca role-run` process group; securely publishes bounded `ca.headless.v2` JSONL artifacts. The clean target integration branch implements and hermetically validates the public command. |
| Optional Ollama provider | `src/providers/ollama.rs`, `src/config.rs` | Per-role `provider`; Rust HTTP only (Hermes→local model for toolful edits is separate; see `docs/HERMES.md`) |
| Optional MLX LM provider | `src/providers/mlx_lm.rs`, `src/providers/openai_compat.rs` | Apple `mlx_lm.server` OpenAI-compat; chat/JSON only — no worktree writes; fail closed when disabled |
| Task import and revision | `src/task_import.rs`, `src/revision.rs`, `schemas/task_import*.json` | Strict single-task preview/apply, canonical digest, immutable origin receipt, clean-tree preconditions, stable repository revision, and pause fingerprinting |
| Mechanistic delegation | `src/mechanistic/*`, `src/verification/registry.rs`, `src/self_improvement/delegation.rs` | One versioned read/observe/verify action contract, bounded tracked-repository queries, dependency-aware package/target evidence planning, deterministic terminal-failure diagnosis, local `ca-watch` observation, policy receipts, and inert learned-manifest authority |
| MCP cockpit | `src/mcp/server.rs`, `src/mcp/jobs.rs` | `rmcp` stdio transport, one pinned canonical Git root, closed tool schemas, zero resources, observe/delegate/operate catalogs, and durable step/delegation jobs |
| Machine JSON mode | `src/machine_json.rs`, `src/verification/runner.rs` | Under `--json`, progress → stderr so stdout stays one receipt |
| Roles | `src/roles/*` | Target home for role runners and decision schemas; role code should not commit or mutate queue state |
| Domain | `src/domain/*` | Task, policy, events, routes, route-learning, derived-status, acceptance-contract, tool-health vocabulary |
| Context | `src/context/*` | Bounded Worker file leases and stale-tolerant repo-hint cache |
| Config | `src/config.rs` | `Config`/role defaults serde shapes; load/save IO remains in `app` |
| State | `src/state.rs` | Persistent routing `State` serde shapes; load/save and transitions remain in `app` |
| Lock | `src/lock.rs` | `.orchestrator/codex-team.lock` for concurrent mutating CLI commands |
| Git / commit authority | `src/git.rs` | Git discovery and status, clean-start checks, runtime exclude installation, explicit staging, centralized auto/manual commit coordination, pending-intent fingerprinting, and exact approval application |
| Telemetry | `src/telemetry/*` | Target home for `.orchestrator/events.jsonl` and cost records (much writing still driven from `app`) |
| Repair | `src/repair.rs` | Worker retry briefs, failure fingerprints, compiler diagnostics |
| Prompts/schemas | `src/prompts.rs`, `prompts/`, `schemas/` | Embedded prompt/schema assets |

## Control Flow

1. CLI parses flags and calls `app` (often after acquiring the repo lock). MCP instead pins one canonical Git root and exposes typed tools through `rmcp`.
2. `app` loads config/state under `.orchestrator/`, rejects a pending commit, and applies clean-start/revision preconditions before recovery or role traffic when configured.
3. A typed external task may enter only through a mutation-free preview followed by an exact digest- and revision-bound apply. Imported tasks stay state-owned and bypass Markdown reconciliation.
4. Rust checks queues, non-AI tasks, stale completion, dirty tree state, and deterministic routes. Typed mechanistic actions validate before routing; repository-query receipts may precompute compact context, and eligible `rust_only` evidence can complete without Worker.
5. If needed, Rust dispatches a role to its configured engine. `prefer` may fall back only with an explicit declined/unavailable receipt, `require` never falls back, and a real evidence failure is terminal evidence. `ca` advisory roles receive fixed read authority; only Worker receives task-path write grants. No engine silently falls back to another.
6. Worker edits only one selected task; Rust detects control-plane mutations and guard violations.
7. Rust runs deterministic verification when applicable, builds evidence, and routes to auto-accept, Senior Dev, Project Manager, or CEO.
8. In legacy auto mode, the central Git coordinator stages and commits the exact accepted paths. In manual mode it records one digest-bound `PendingCommit` and pauses with HEAD/index unchanged; a later exact `commit-pending` call revalidates and commits.
9. MCP `repo_query` runs synchronously without raw-result persistence. `delegation_start` creates a durable evidence or local-observation job; cancellation affects only executor-owned children/watchers. `step_start` launches one revision-bound `codex-team step --json` subprocess and immediately returns a durable job ID.

## Data Flow

- Human-facing control docs: `ROADMAP.md`, `NEXT_STEPS.md`, `NEXT_STEPS_non-ai.md`, `AGENTS.md`.
- Runtime state: `.orchestrator/state.json`, lock, prompts, schemas, task cards, run logs, evidence, recipes, events, cost ledger, immutable imports, and durable MCP jobs.
- Manual intent: `State.pending_commit` binds source lane, optional task/import identity, base HEAD, exact dirty paths, per-path fingerprints, message, evidence identifiers, and a deferred Rust transition.
- `ca` artifacts: bounded `.orchestrator/runs/*.ca.stdout.jsonl` and `*.ca.stderr.log`; diagnostics are sanitized and do not authorize later guard/commit work.
- MCP jobs: atomic `.orchestrator/mcp/jobs/<job-id>.json`; job observation records are excluded from repository revisions so polling does not invalidate an approval.
- Prompt/schema sources: `prompts/` and `schemas/` are embedded through `src/prompts.rs` and refreshed into `.orchestrator/` on role calls.
- Queue parser output becomes `TaskSpec` values from `src/domain/task.rs`.

## Boundaries and Invariants

- Queue parsing should stay pure and not call Codex, run commands, or mutate files.
- Verification should validate commands and execute without shell interpretation; `src/verification/runner.rs` is the command execution boundary.
- Codex calls spawn through `src/codex/run.rs`; `ca` calls spawn through `src/ca/run.rs`. `src/app.rs` builds prompts, selects explicit role authority, and parses typed role outputs.
- `ca` is an execution engine, not a scheduler. The runner never grants shell, Git, delete, MCP, browser, jobs, subagents, skills, memory, or external communication, and Rust's post-run guard remains authoritative.
- Manual mode must not stage or commit before a separate exact approval. A pending intent blocks later work, and stale approval must leave project files and the pending record intact.
- Imported tasks never derive authority from caller-supplied status/automation values and never cause project Markdown to be parsed or rewritten as their origin of truth.
- MCP tool arguments never select a repository. Observe has 11 exact read-only tools including `repo_query`; delegate adds four exact read/observe/verify job tools; operate adds 13 exact orchestration/learning mutation tools only after manual/clean/cancellable startup checks.
- Learned package files are inert data. Only one valid digest-bound `DELEGATION.json` can propose exact typed actions; script text, shell strings, binaries, executable members, ambiguous matches, repository drift, quarantine, retirement, or human correction prevent automatic authority.
- Guard reports are computed from Rust snapshots, not model claims.
- Route scorecards, positive route learning, derived status, and tool-health cooldowns are advisory unless converted into explicit Rust routing policy.
- AI roles do not run git or mutate durable state directly. PM and Senior Dev may authorize typed Rust commit requests; Rust still owns staging, commits, queue status updates, and roadmap completion truth.
- Legacy Intern config is accepted for compatibility but ignored at runtime.
- Watch mode remains lower priority than queue/verification integrity.

## Test Mapping

- Queue parser: `cargo test queue::next_steps`.
- Legacy Intern/support packets: `cargo test legacy_intern_support_contract`.
- Fake Codex runner routing: `cargo test step_once_routes_planner_through_fake_codex_runner`.
- Codex adapter/parser: `cargo test codex`.
- Restricted `ca` adapter: `cargo test ca::run::tests` (includes exact argv/JSONL and fake descendant cleanup on Unix).
- Commit/manual intent: `cargo test git::tests`.
- Import/revision/`ca` recipes: `cargo test task_import`; `cargo test revision`; `cargo test verification::ca`; `cargo test --test ca_integration_foundations`.
- MCP SDK/jobs: `cargo test mcp::`; `cargo test --test mcp_sdk`.
- Mechanistic delegation and learned manifests: `cargo test mechanistic::`; `cargo test self_improvement::delegation`.
- Locks: `cargo test lock`.
- Leases: `cargo test lease`.
- Strict schemas: `cargo test role_schemas`.
- Full suite: `cargo test`.

## Safe Edit Routing

See `docs/AGENT_MAP.md` for the full table. Highlights:

- CLI flags/help: `src/cli.rs`, then `README.md`.
- Task-card parsing: `src/queue/next_steps.rs`.
- Deterministic command policy: `src/verification/recipe.rs`, `src/verification/runner.rs`.
- Typed repository queries/evidence/observation: `src/mechanistic/*`; shared command registry in `src/verification/registry.rs`.
- Guard/scope: `src/guard/README.md`.
- Prompt/schema text: `prompts/`, `schemas/`, `src/prompts.rs`.
- Codex process invocation: `src/codex/run.rs`.
- Restricted `ca` process invocation: `src/ca/run.rs`; configuration in `src/config.rs`.
- Typed import/revision: `src/task_import.rs` / `src/revision.rs`; schemas in `schemas/task_import*.json`.
- Commit mode/pending approval: `src/git.rs`, DTOs in `src/state.rs`, CLI/receipts in `src/app.rs` and `src/app/receipts.rs`.
- MCP tools/jobs: `src/mcp/server.rs` / `src/mcp/jobs.rs`.
- Repo lock: `src/lock.rs`.
- Context leases: `src/context/lease.rs`.
- Config/State DTOs: `src/config.rs` / `src/state.rs`.
- Runtime path construction: `src/paths.rs`.

## Files to Avoid Unless Specifically Tasked

- `src/app.rs`: central conductor; broad edits are high-risk.
- `schemas/*.json`: prompt contract changes can break role parsing.
- `prompts/*.md`: prompt changes should be paired with schema/test review.
- `.orchestrator/`: runtime state; ignored by Git and not part of source docs.

## Known Architectural Gaps

- Verification plan construction and evidence persistence orchestration still largely live in `src/app.rs` (DTOs extracted).
- Live Codex terminal rendering still lives mostly in `src/app.rs`; `src/codex/live.rs` remains the target home.
- Config/State DTOs are extracted; load/save and most transition effects remain in `src/app.rs`.
- ORCH-CA-001 is not production-complete. The clean target branch implements the strict public `ca role-run` contract and passes hermetic real-process compatibility; the original dirty `/Users/aji/project/ca` checkout remains read-only and outside the implementation/pilot boundary.
- Real `ca` execution passes all four role schemas plus allowed/denied Worker writes, and actual `ca::McpManager` observe/operate discovery passes. Final post-hardening repository matrices, supervised one-step CLI/MCP dogfood, and live-provider use remain incomplete; live traffic requires separate authorization.
- MCP operate-mode cancellation, restart, and partial-state handling have focused process-group coverage, including live orphan termination on server restart. The final aggregate matrix remains a completion gate.
- CI exists (`.github/workflows/ci.yml`); clippy is not yet enforced with `-D warnings`.
