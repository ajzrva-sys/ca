# Hermes Integration (CLI contract + MCP implementation preview)

Target: **Nous Research Hermes Agent** as boss scheduler alongside Rust `codex-team` and local MLX.

The established Hermes path remains the bounded CLI supervisor described
below. ORCH-CA-001 now also contains an SDK-backed `codex-team mcp` server, but
that surface is pilot-only until the current integration branches pass their
final matrices and supervised external-client pilot. Actual `ca::McpManager`
compatibility now passes hermetically, but that is not production MCP evidence.

**Human operator guide:** [`HERMES_IMPLEMENTATION_GUIDE.md`](HERMES_IMPLEMENTATION_GUIDE.md)
**Contract export (optional mirror):** `skills/codex-team/SKILL.md` + `skills/codex-team-boss/SKILL.md` — **deleted** (were optional contract exports; Hermes owns living skills under `~/.hermes/skills/`)
**Asks → truth map:** [`HERMES_INTEGRATION_CHECKLIST.md`](HERMES_INTEGRATION_CHECKLIST.md)
**Loop script:** [`../scripts/hermes_supervisor_loop.sh`](../scripts/hermes_supervisor_loop.sh)
**Install contract export:** [`../scripts/install_hermes_skill.sh`](../scripts/install_hermes_skill.sh) (optional seed; Hermes may overwrite)

## Triad model (operator-locked)

| Actor | Owns | Does not own |
|---|---|---|
| **Hermes** | Boss scheduling; **direct MLX control**; **skill authorship** in `~/.hermes/skills/` | Rust state.json / commits when using the Rust lane incorrectly (see below) |
| **Rust (`codex-team`)** | Admit/guard/verify/state/Git mechanics **when Hermes chooses the Rust lane**; passive learning; **deterministic zero-AI** automation. Manual mode separates human approval from commit execution. | Writing Hermes skills unsolicited; policing Hermes's direct MLX use |
| **MLX** | Local inference for **Hermes tasks** and for **Rust roles** (`provider = "mlx_lm"`) | Repo authority without a chosen lane |

**Cost / preference ladder (high → low spend):**

1. Cursor / cloud agents (expensive)
2. Hermes (coordinator; may drive MLX)
3. **Rust deterministic** — $0 and **fastest** (preferred sink for repeatable work)
4. **MLX** — $0 and **slower** (local AI when judgment is still needed)

Rust's unique value is moving work **down** into deterministic automation, not inventing Hermes skills.

### Dual MLX lanes

- **Hermes → MLX:** Hermes already controls MLX directly (`custom:mlx`, skills, chats). Valid for Hermes-owned tasks.
- **Rust → MLX:** When Hermes schedules a **Rust lane** cycle, role models use `roles.*.provider = "mlx_lm"` in `.orchestrator/config.toml` so receipts/evidence/learning accumulate under Rust.

### Edit routing (this machine — ORCH-MLX-WRITE-001)

Dogfood (`HERMES_MLX_DOGFOOD.md`) locked the policy:

| Work | Preferred lane | Why |
|---|---|---|
| Mechanical verify / command-only | **Rust deterministic** | $0, fastest; no model |
| Local AI must **edit** files | **Hermes → MLX (tools/skills)** | Only toolful path on this Codex-free machine |
| Rust cycle role calls (PM/Senior/Worker) | **Rust → MLX** chat/JSON | Feeds receipts/learning; **does not write the worktree** |
| Toolful Worker via Codex | N/A here | No Codex binary; do not expect it |

**Hard rule for Hermes:** never treat a Rust-lane `Worker` `status: "done"` + `files_changed` as proof that files exist. Rust Guard blocks missing claimed paths; actual edits belong on Hermes→MLX (or Cursor), then optionally re-admit through Rust for verify/commit.

Rust patch-apply from Worker JSON remains a deferred ExecPlan (not this slice).

### Skill authorship

1. Ask Hermes what it needs to skill-up for `codex-team`.
2. Rust answers with **facts only**: CLI, receipts, schemas, `MACHINE_ACCOMPLISHMENTS.yaml`, doctor/providers JSON.
3. Hermes writes/updates `~/.hermes/skills/...` itself.
4. In-repo `skills/` were optional **contract exports** (now deleted). Do not treat Cursor/Rust edits of those files as living Hermes skills unless Hermes requests a seed and then regenerates its copy.

## Hermes ↔ Rust contract

### Hermes needs from Rust

| Need | How Rust delivers |
|---|---|
| Admit/guard/verify/commit/state (Rust lane) | `codex-team step --repo "$REPO" --json </dev/null` |
| Preflight health | `codex-team doctor --repo "$REPO" --json` |
| Provider inventory | `codex-team providers --repo "$REPO" --json` |
| Queue/status snapshot | `codex-team status --repo "$REPO" --json --observe` |
| MCP observation (pilot only) | `codex-team mcp --repo "$REPO" --access observe`; tools: `status`, `doctor`, `step_status`; resources: none |
| Human disposition recording | `codex-team disposition --repo "$REPO" --decision <choice> --json` |
| Machine-readable outcomes | One JSON object on stdout under `--json` (`protocol_version: 1`) |
| Cycle outcome branching | `outcome`: `continue`, `stopped`, `paused`, `no_work`, `error` |
| Error details | `error_kind`, `error_summary` when `outcome: error` |
| Learning signals (passive Rust memory) | `learning`: `recommended_lane`, `cooldowns`, `recent_episode_error_kinds`, `top_signals` |
| Event trail | `events_path` → `.orchestrator/events.jsonl` |
| Exit codes | `0` parse outcome; `1` failure; `2` usage/pause-sentinel |
| Schemas / catalogs on demand | `schemas/*_receipt.schema.json`, `MACHINE_ACCOMPLISHMENTS.yaml` |

Skill creation: Hermes asks for gaps; Rust supplies the table above / JSON samples; **Hermes writes** `~/.hermes/skills/`.

### Rust needs from Hermes (Rust lane only)

| Need | Hermes rule when using Rust lane |
|---|---|
| Never write control plane | Don't touch `.orchestrator/state.json`, prompts, schemas, lock files |
| Never commit | Don't run `git add` / `git commit` for that lane |
| Manual commit approval | In `commit_mode = "manual"`, ask the human to review the exact pending ID/digest/revision before `commit_pending`; never approve automatically |
| Don't impersonate Rust roles | Don't `delegate_task` as Worker/PM/Senior for admit/verify/commit |
| Respect locks | One Rust supervisor process per repo at a time |
| Close stdin on step | Always `</dev/null` |
| Cap cycles | Max 5 automatic `step` calls; never infinite-loop |
| Pause = stop | On `outcome: paused`, inspect `pause` and `pending_commit`, notify the human, and do not invent a disposition or commit approval |
| Don't claim done without Rust | Only Rust receipt / queue status confirms Rust-lane completion |
| Parse, don't scrape | Read JSON receipts; don't scrape stderr |

## Hermes Agent Supervisor Setup (Rust lane)

Copy-paste on a machine that already has `hermes` and `codex-team` on `PATH`:

```sh
REPO="$(pwd)"   # or supervised target repo
ORCH=/path/to/codex_team_orchestrator

# Optional: seed contract export (Hermes may replace)
bash "$ORCH/scripts/install_hermes_skill.sh"

codex-team doctor --repo "$REPO" --json
codex-team step --repo "$REPO" --json </dev/null
bash "$ORCH/scripts/hermes_supervisor_loop.sh" "$REPO" 5

hermes chat -s codex-team,codex-team-boss -q "Supervise $REPO: doctor then status; choose Rust vs direct MLX by cost ladder"
```

There is no Hermes `supervisor.enabled` config key; supervision is CLI + Hermes-owned skills.

Named providers in `~/.hermes/config.yaml` (e.g. `custom:mlx`) are Hermes-owned. When using the **Rust lane**, enable `[mlx_lm]` / `[ollama]` and `roles.*.provider` in the supervised repo's `.orchestrator/config.toml`.

### Provider wiring (this machine, after ORCH-LOCAL-MLX-001)

```
roles.master/planner/foreman/worker: provider=mlx_lm
model: mlx-community/Qwen3-Coder-30B-A3B-Instruct-4bit
mlx_lm: enabled=true
codex: skipped when no role uses provider=codex
```

Example TOML is in [`HERMES_MLX_DOGFOOD.md`](HERMES_MLX_DOGFOOD.md). Preflight: `doctor --json` / `providers --json`.

## Rust lane invariants

When Hermes chooses the **Rust** lane for a supervised repo:

- Do not write `.orchestrator/state.json`, prompts, schemas, or lock files from Hermes.
- Do not run `git add` / `git commit` for that lane — Rust commits.
- Prefer `codex-team … --json` over nesting Hermes `delegate_task` as Worker/PM/Senior for admit/verify/commit.
- Observe outcomes from receipts and `events.jsonl`. Under `--json`, **stdout is one JSON object**; progress is on stderr.
- Rust learns **passively** from episodes / tool_health / scorecards whether or not Hermes reads them.
- JSON receipts are the Rust-lane contract; schemas in `schemas/` are canonical.
- Human remains disposition authority for Rust-lane pauses.
- In manual mode, human also remains pending-commit authority. Hermes may show
  the exact pending intent but must not call `commit-pending` or the MCP
  `commit_pending` tool without a fresh explicit decision.

Hermes **may** use direct MLX outside or beside that lane; that is not a Rust doc prohibition.

## Exit codes

| Code | When | Supervisor action |
|---|---|---|
| `0` | Command completed without Rust error. Cycle/status/disposition success always exits `0` even if `outcome` is `paused`/`stopped`/`no_work`. | Parse JSON; branch on `outcome` / pause fields |
| `1` | Lock contention, I/O, missing init, invalid decision, provider failure, **`step --json` with `outcome: "error"`** (receipt already on stdout), **or** `doctor --json` with `healthy: false` | Do not treat as a successful cycle; read stderr and JSON `error_*` / doctor checks |
| `2` | Clap usage errors; also used by `scripts/hermes_supervisor_loop.sh` when `outcome=paused` | Fix flags / notify human |

## Supervisor POC (CLI)

```sh
codex-team step --repo /path/to/repo --json </dev/null
codex-team status --repo /path/to/repo --json
codex-team status --repo /path/to/repo --json --observe
codex-team disposition --repo /path/to/repo --decision preserve --json
codex-team doctor --repo /path/to/repo --json
codex-team providers --repo /path/to/repo --json
```

Receipt schemas: `schemas/cycle_receipt.schema.json`, `schemas/status_receipt.schema.json`, `schemas/disposition_receipt.schema.json`, `schemas/doctor_receipt.schema.json`. Machine catalog: [`MACHINE_ACCOMPLISHMENTS.yaml`](MACHINE_ACCOMPLISHMENTS.yaml). MLX via Rust roles: [`HERMES_MLX_DOGFOOD.md`](HERMES_MLX_DOGFOOD.md).

`step --json` includes `cycle_id`, `outcome`, additive `pause` / `error_*` / `provider_hint` / `learning`, and `events_path`.

## MCP implementation preview (ORCH-CA-001)

The integration branch runs the standard SDK server through the main binary:

```sh
codex-team mcp --repo /canonical/git/root --access observe
```

The repository is canonicalized and pinned at startup; no tool accepts a repo
argument. `resources/list` returns an empty list. Tool argument schemas reject
unknown properties and failures are structured tool results.

| Access | Tools |
|---|---|
| `observe` (default) | `status`, `doctor`, `step_status` |
| `operate` | Observe tools plus `task_import_preview`, `task_import_apply`, `step_start`, `step_cancel`, `commit_pending`, `disposition` |

Operate startup requires an initialized repository with
`commit_mode = "manual"`, `require_clean_start = true`, and only cancellable
Codex/`ca` role engines. `step_start` is revision-bound and asynchronous: call
it once, then poll `step_status` using its durable job ID. `step_cancel` is the
only retry-safe mutation. Task apply, pending commit, and disposition each
require their exact digest/revision/fingerprint preconditions.

This is not yet the recommended Hermes production path. Generic `rmcp` SDK
coverage and an actual `ca::McpManager` observe/operate compatibility test now
pass against disposable repositories, and the clean target branch implements
`role-run`. Those hermetic gates are not a supervised Hermes pilot or a
live-provider result. Continue to use the bounded CLI supervisor until
`PLAN.md` records the final matrices and dogfood gates as passing.

## Remaining phases

| Phase | Approach |
|---|---|
| 2 | Stable JSON contracts + strict digest-bound task import (implemented on ORCH-CA-001; final validation pending) |
| 3 | SDK-backed observe/operate MCP with asynchronous jobs (implemented; actual `ca` client compatibility passes; supervised pilot pending) |
| 4 | Optional Hermes Worker provider behind a runner trait — only after Phase 2 stabilizes |

See also `docs/OPERATIONS.md` and `IDEA.md`.
