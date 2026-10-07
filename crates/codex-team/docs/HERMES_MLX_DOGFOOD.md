# MLX LM dogfood notes

Recorded during ORCH-HERMES-003 roadmap pack; live run completed as **ORCH-MLX-DOGFOOD-001** (2026-07-14).

## Goal

Prove **Rust-owned** `provider = "mlx_lm"` role calls (especially Worker) against a live `mlx_lm.server`, with Hermes scheduling CLI only (Hermes→MLX remains allowed separately for toolful work).

## Recommended config (this machine / all-MLX)

```toml
[mlx_lm]
enabled = true
base_url = "http://127.0.0.1:8080"

[roles.worker]
provider = "mlx_lm"
model = "mlx-community/Qwen3-Coder-30B-A3B-Instruct-4bit"
timeout_seconds = 600
```

Live catalog (operator 2026-07-14, `GET http://127.0.0.1:8080/v1/models`): `Outlier-Ai/Qwen2.5-Coder-32B-Instruct-MLX-4bit`, `mlx-community/Qwen3-Coder-30B-A3B-Instruct-4bit`, `Jiunsong/supergemma4-e4b-abliterated-mlx`.

Preflight:

```sh
codex-team doctor --repo "$REPO" --json
codex-team providers --repo "$REPO" --json
```

## Disposable loop

1. Throwaway git repo (not the orchestrator tree). Example: `~/tmp/codex-team-mlx-dogfood-001`.
2. `Status: Approved` Worker card (not `Ready`).
3. `step --json` with stdin closed (`</dev/null`) or `scripts/hermes_supervisor_loop.sh`.
4. On `outcome: "error"`, parse `error_kind` / `error_summary`; do not treat empty stdout as success.

## ORCH-MLX-DOGFOOD-001 result (2026-07-14)

Repo: `/Users/andrewzimmerman/tmp/codex-team-mlx-dogfood-001`

| Check | Result |
|---|---|
| doctor `--json` healthy | Pass (`mlx_lm` reachable, 3 models, Codex skipped) |
| providers `--json` | Pass (all roles `mlx_lm`) |
| Worker HTTP via `mlx_lm` | Pass (~10–40s/role call) |
| Missing `task_id` in Worker JSON | Fail → fixed: Rust stamps known `task_id` |
| False `done` without writing files | Fail → sealed: Guard blocks missing `files_changed` paths |
| `--json` stdout purity | Fail (runner `println` + `git commit`) → fixed for runner progress + accepted commits under machine JSON |
| STATUS.md actually written by mlx Worker | Fail (chat-only provider; no tool write lane) |

Receipts / logs:
- `/tmp/mlx-dog-doctor.json`, `/tmp/mlx-dog-providers.json`
- `/tmp/mlx-dog-step1.json` — `outcome=error`, `error_kind=malformed_output` (missing `task_id`)
- `/tmp/mlx-dog-step2.json` — false accept before integrity seal (STATUS.md never existed; only NEXT_STEPS marked Done)
- `/tmp/mlx-dog-step3.json` — `outcome=stopped`; Guard blocked `MLX-DOG-002`; stdout is a single JSON object

## Lane implication (ORCH-MLX-WRITE-001 decision)

**Preferred toolful edit lane on this machine: Hermes → MLX (tools/skills).**

| Lane | Edits worktree? | Use for |
|---|---|---|
| Rust deterministic | Yes (commands only) | Mechanical verify / command-only evidence |
| Hermes → MLX | Yes (via Hermes tools) | Local AI file edits (preferred) |
| Rust → MLX Worker/roles | **No** (chat/JSON only) | Planning/review JSON → Rust admit/learning |
| Codex Worker | Yes (when installed) | Not available on this machine |

Hard rule: do **not** expect `roles.worker.provider = "mlx_lm"` to create or patch files. Guard will block false `done` when claimed paths are missing.

Rust-owned patch-apply from Worker JSON is deferred (future ExecPlan only).

## Known brittleness (mitigated)

| Failure | Mitigation |
|---|---|
| `null` optional strings (`blocker`, `cwd`, `summary`) | `src/providers/json_hydrate.rs` |
| Missing Worker `task_id` | `stamp_known_task_id` from Rust-known task |
| Claimed `files_changed` missing on disk | `enforce_worker_claimed_file_evidence` |
| Invented `review_tier` like `automated` | Mapped to Rust enums |
| Hard step failure without JSON | `outcome: "error"` receipt under `--json` |
| Progress on `--json` stdout from verification runner | Shared `machine_json` mode → stderr |
| Empty-queue Planner churn | Idle stop when no unfinished/approved work |
| Repeated local parse/unreachable failures | `[routing].local_provider_failure_budget` |
