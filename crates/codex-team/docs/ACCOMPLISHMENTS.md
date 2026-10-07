# Machine Accomplishments (ORCH-HERMES / OLLAMA / MLX)

Canonical **machine-parseable** catalog: [`MACHINE_ACCOMPLISHMENTS.yaml`](MACHINE_ACCOMPLISHMENTS.yaml).

This page is a human index into that YAML. Prefer the YAML for automation.

## Shipped tasks

| ID | Status | Contract |
|---|---|---|
| `ORCH-HERMES-003` | done | Error receipts, hydrate, idle stop, providers JSON, MLX dogfood notes |
| `ORCH-HERMES-002` | done | Installable skill, exit codes, setup block, checklist map |
| `ORCH-HERMES-001` | done | Quiet `--json` CLI receipts, schemas, skill, loop script |
| `ORCH-OLLAMA-001` | done | Rust `provider=ollama` → local Ollama HTTP |
| `ORCH-MLX-001` | done | Rust `provider=mlx_lm` → `mlx_lm.server` HTTP |

## Parse rules for supervisors

1. Under `--json`, parse **entire stdout** as one JSON object (`protocol_version: 1`).
2. Progress/logs are on **stderr** only.
3. Close stdin for `step`: `</dev/null`.
4. Never let Hermes `delegate_task` / talk to Ollama or MLX for supervised repos; configure `roles.*.provider` so Rust invokes locals.

## Schema map

| Receipt / config | Schema |
|---|---|
| `step --json` | [`schemas/cycle_receipt.schema.json`](../schemas/cycle_receipt.schema.json) |
| `status --json` | [`schemas/status_receipt.schema.json`](../schemas/status_receipt.schema.json) |
| `disposition --json` | [`schemas/disposition_receipt.schema.json`](../schemas/disposition_receipt.schema.json) |
| `doctor --json` | [`schemas/doctor_receipt.schema.json`](../schemas/doctor_receipt.schema.json) |
| Provider TOML shape (docs) | [`schemas/provider_config.schema.json`](../schemas/provider_config.schema.json) |

## CLI cheat sheet

```sh
codex-team doctor --repo "$REPO" --json
codex-team status --repo "$REPO" --json --observe
codex-team step --repo "$REPO" --json </dev/null
codex-team disposition --repo "$REPO" --decision preserve|recheck|gate|keep-paused --json
scripts/install_hermes_skill.sh
scripts/hermes_supervisor_loop.sh "$REPO" 5
```

Exit codes: see [`HERMES.md`](HERMES.md) § Exit codes. Asks→truth: [`HERMES_INTEGRATION_CHECKLIST.md`](HERMES_INTEGRATION_CHECKLIST.md).

## Local providers (Rust-owned)

```toml
[ollama]
enabled = true
base_url = "http://127.0.0.1:11434"

[mlx_lm]
enabled = true
base_url = "http://127.0.0.1:8080"

[roles.worker]
provider = "ollama"   # or "mlx_lm" / "codex"
model = "qwen2.5-coder:32b-hermes"
timeout_seconds = 600
```

Fail-closed: if `provider` is `ollama`/`mlx_lm` but the matching `enabled` is false, the role call errors (no Codex fallback).

## Active implementation preview (not shipped)

- ORCH-CA-001 adds supervised manual commits, digest-bound task import, a
  restricted orchestrator-side `ca` runner, and SDK-backed observe/operate MCP.
- The current machine catalog intentionally does not mark it shipped: the clean
  `ca` branch now implements `role-run`, and real `RealCaRunner` plus actual
  `ca::McpManager` compatibility pass, but final post-fix matrices and
  supervised/live pilots are not complete. Live traffic remains unauthorized.

## Deferred (explicit)

- Hermes-as-Worker provider
- Auto-routing to Ollama/MLX via scorecards
- Silent Codex fallback on local HTTP failure

## Related docs

- [`HERMES.md`](HERMES.md) — short contract
- [`hermes-skill-draft.md`](hermes-skill-draft.md) — standing instructions
- [`HERMES_IMPLEMENTATION_GUIDE.md`](HERMES_IMPLEMENTATION_GUIDE.md) — human POC
- ExecPlans: `docs/exec-plans/completed/orch-hermes-001-*.md`, `orch-ollama-001-*.md`
