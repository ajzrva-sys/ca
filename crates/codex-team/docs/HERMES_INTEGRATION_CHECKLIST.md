# Hermes integration checklist (asks → repo truth)

Maps what Hermes asked for during ORCH-HERMES-002 against what `codex-team` actually ships. Prefer receipt JSON and schemas over this prose when they differ.

## P0 asks

| Hermes ask | Repo truth | Status |
|---|---|---|
| Exact JSON schemas for `step`/`status`/`disposition`/`doctor --json` | `schemas/cycle_receipt.schema.json`, `status_receipt.schema.json`, `disposition_receipt.schema.json`, `doctor_receipt.schema.json` (`protocol_version: 1`). Typed builders in `src/app/receipts.rs`. | **Done** |
| Exit codes + doctor unhealthy behavior | See `docs/HERMES.md` § Exit codes. Success for cycle commands is exit `0` + parse `outcome`. Hard failures under `step --json` emit `outcome: "error"` then exit `1`. `doctor --json` prints receipt then exits `1` when `healthy: false`. Clap misuse → `2`. | **Done** |
| Skill install path + name | Skill name `codex-team`. Source: living `~/.hermes/skills/codex-team/` (Hermes-owned; in-repo exports deleted). Install: `scripts/install_hermes_skill.sh`. | **Done** |
| Copy-paste Hermes supervisor setup | `docs/HERMES.md` § Hermes Agent Supervisor Setup | **Done** |

## P1 asks

| Hermes ask | Repo truth | Status |
|---|---|---|
| `--json` on all subcommands | JSON is still not universal. In addition to `step`, `status`, `disposition`, and `doctor`, ORCH-CA-001 adds typed JSON for task import, pending commit, and built-in verification; provider/route commands retain their documented surfaces. Do not scrape human-only output. | **Partial** |
| MCP tool names | ORCH-CA-001 implements `codex-team mcp`. Observe: `status`, `doctor`, `step_status`. Operate additionally: `task_import_preview`, `task_import_apply`, `step_start`, `step_cancel`, `commit_pending`, `disposition`. Resources are always empty and the repo is pinned at startup. | **Implemented, pilot-only** |
| Echo receipt / `--receipt` flag | No opaque receipt token. State is authoritative under the repo lock. `cycle_id` correlates `events.jsonl` only. | **N/A — use state** |
| Evidence format on disposition | Disposition receipt fields: `outcome`, `decision`, `refresh_report`, `human_disposition_required`, `pause`, `events_path`. Evidence lives under `.orchestrator/` + events; not a sealed proof path list on the receipt. | **Partial — use events_path** |

## P2 asks

| Hermes ask | Repo truth | Status |
|---|---|---|
| Doctor check `severity` | Each check has `severity`: `ok` \| `error` (mirrors `ok`). No `warning`/`critical` yet. | **Done (v1)** |
| Idempotency of `step` | **Not** guaranteed idempotent. Do not blindly retry after partial cycle. Lock + events tell truth. | **Documented** |
| Env assumptions | `codex-team` on `PATH` (or `CODEX_TEAM` override for scripts), `$HOME/.hermes/skills`, closed stdin for `step`, no API keys required for CLI admit itself. Role calls may need Codex or local `[ollama]`/`[mlx_lm]`. | **Documented** |

## Assumption confirmations

| Assumption | Answer |
|---|---|
| `step` with stdin closed never hangs waiting for input | **Yes** for human-disposition path: non-TTY/`</dev/null` stops with `paused` instead of prompting. Always pass `</dev/null`. |
| `status --json` is read-only | Use `status --json --observe` for no queue recover/save. Still acquires the repo lock. Default `status --json` may refresh derived queue under lock. |
| `disposition --json` is the terminal “done” transition | **No.** It clears/resolves a **human disposition pause** only (`preserve`\|`recheck`\|`gate`\|`keep-paused`). Project completion is a separate state/`outcome` path. |

## Still deferred

- Final ORCH-CA-001 post-fix matrices and supervised CLI plus observe/operate
  external-client dogfood; clean `ca` role-run and actual `ca::McpManager`
  compatibility already pass hermetically
- Operate MCP production use, pending supervised cancellation/stale-state
  dogfood and an explicit human approval workflow
- Hermes-as-Worker provider
- Silent Codex fallback when local providers fail (intentionally fail-closed)
