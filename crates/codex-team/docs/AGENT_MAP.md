# Agent Edit Map

Use this after `AGENTS.md` and `ARCHITECTURE.md` when choosing which file to open first.
Prefer the smallest owning module. Stop and remount if the task requires editing `src/app.rs` for unrelated behavior.

## Task → files

| If you need to… | Start here | Also check | Stop / avoid unless tasked | Test filter |
|---|---|---|---|---|
| Change CLI flags / subcommands | `src/cli.rs` | `README.md` | queue/verification internals | `cargo test` (CLI via clap compile) |
| Parse / mutate `NEXT_STEPS` task IDs or Status | `src/queue/next_steps.rs` | `src/queue/README.md` | inventing substring ID matches | `cargo test queue::next_steps` |
| Change verification allowlist / recipes | `src/verification/recipe.rs`, `src/verification/runner.rs` | `src/verification/README.md`, allowlist in `src/app.rs` | shell pipelines | `cargo test verification` / `recipe` |
| Change ValidatedCommand / cwd / timeouts | `src/verification/runner.rs` | `src/codex/run.rs` (Codex process-group kill) | bypassing allowlist | `cargo test verification` |
| Change Worker file-scope checks | `src/guard/*` | `src/guard/README.md` | trusting model `files_changed` | `cargo test` guard-related filters |
| Change role prompts / schemas | `prompts/`, `schemas/`, `src/prompts.rs` | pairing prompt↔schema strictness tests | editing `.orchestrator/` copies as source of truth | `cargo test role_schemas` / `worker_schema` |
| Change Codex spawn / run artifacts | `src/codex/run.rs` | `src/codex/README.md`, `src/codex/stream.rs` | spawning `codex` outside the runner seam | `cargo test codex` |
| Change retry fingerprints / no-progress stops | `src/repair.rs` | CHURN helpers in `src/app.rs` | silent infinite retries | `cargo test worker_retry` |
| Change context leases / repo hints | `src/context/lease.rs` | `src/context/README.md`, Worker `expand_lease` in schema | embedding global repo maps by default | `cargo test lease` |
| Change repo lock / concurrent CLI safety | `src/lock.rs` | CLI entrypoints in `src/app.rs` | unlocking mutating commands without tests | `cargo test lock` |
| Change Config TOML shape / defaults | `src/config.rs` | load/save still in `src/app.rs` | inventing knobs not wired into routing | focused config tests / `cargo test` |
| Change `state.json` routing flags | `src/state.rs` | load/save + transitions in `src/app.rs` | treating derived status as authority | `cargo test` |
| Change Git root / future git adapter | `src/git.rs` | commit staging still mostly in `src/app.rs` | absorbing unrelated dirty paths into commits | acceptance/commit tests |
| Change acceptance contracts / characterization gate | `src/domain/acceptance.rs` | `docs/OPERATIONS.md` | launching Worker without required evidence | `cargo test acceptance_contract` |
| Change tool-health / derived status / route scorecards | `src/domain/tool_health.rs`, `derived_status.rs`, `route_learning.rs` | `docs/OPERATIONS.md` | treating advisory telemetry as hard blocks (except documented stops) | `cargo test tool_health` / `derived_status` / `route_learning` |
| Hermes supervisor JSON receipts | `src/cli.rs`, `step`/`status`/`disposition`/`doctor` in `src/app.rs`, `src/app/receipts.rs` | `docs/HERMES.md`, `docs/HERMES_INTEGRATION_CHECKLIST.md`, `schemas/*_receipt.schema.json` | letting Hermes write state or commit; polluted `--json` stdout | `cargo test hermes_` |
| Optional Ollama / MLX LM role providers | `src/providers/ollama.rs`, `src/providers/mlx_lm.rs`, `src/config.rs`, dispatch in `run_role` | `docs/HERMES.md` (edit routing; Hermes→MLX tools OK) | Impersonating Worker/PM via Hermes `delegate_task` for Rust-lane admit/verify/commit; silent Codex fallback | `cargo test ollama` / `cargo test mlx_lm` |
| Restricted `ca` role engine | `src/ca/run.rs`, `src/config.rs`, provider-neutral `run_role` dispatch in `src/app.rs` | `docs/CA_INTEGRATION.md`, root `PLAN.md` | production enablement before final matrices/pilots; widening grants; fallback; treating hermetic coverage as live-provider evidence | `cargo test ca::run::tests` then full suite; invoke the ignored `CA_BIN` real-binary gate explicitly |
| Manual commit / task import / revision | `src/git.rs`, `src/task_import.rs`, `src/revision.rs`, `src/state.rs`, CLI/receipts in `src/app.rs` | `docs/CA_INTEGRATION.md`, `schemas/task_import*.json` | staging before approval; stale approval mutation; reconciling imported tasks against Markdown | `cargo test git::tests`; `cargo test task_import`; `cargo test revision`; integration foundation test |
| MCP observe/operate tools | `src/mcp/server.rs`, `src/mcp/jobs.rs`, `src/cli.rs` | `docs/CA_INTEGRATION.md`, `docs/HERMES.md` | repository override; synchronous long step; retrying start; auto commit/disposition; noncancellable operate provider | `cargo test mcp::`; `cargo test --test mcp_sdk` |
| CI gates | `.github/workflows/ci.yml` | `docs/QUALITY.md` GRD-001 | claiming clippy `-D warnings` is green | CI / local fmt+test |
| Cycle routing / accept / commit (high risk) | `src/app.rs` (read conductor map first) | `docs/AGENT_MAP.md` + ExecPlan | broad unrelated edits in the monolith | focused filter then `cargo test` |

## Conductor regions in `src/app.rs`

Open the file header section map, then jump by banner:

1. Public CLI entrypoints (`init` / `step` / `run` / `status` / …)
2. `step_once*` cycle phases
3. Worker → control-plane/guard → verify → accept/commit
4. Queue recovery / promotion / deterministic lanes
5. Markdown status mutators (exact task-ID + single Status)
6. Verification recipe parsing / allowlist
7. Live terminal rendering (presentation only)
8. Guard snapshots / persistence helpers
9. Prompt/schema seed writes into `.orchestrator/`

## Authority reminders

- Rust owns admission, guard, verification, state transitions, and commits.
- AI output is advisory until converted into typed state, evidence, or commits.
- Prefer ExecPlans for multi-file integrity, verification, or routing changes.
