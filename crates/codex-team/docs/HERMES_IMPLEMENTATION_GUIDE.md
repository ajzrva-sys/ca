# Hermes Implementation Guide (for you)

This is the human operator guide for wiring **Nous Research Hermes Agent** to `codex-team`.  
For the short agent-facing contract, see [`HERMES.md`](HERMES.md). For crash/lock recovery, see [`OPERATIONS.md`](OPERATIONS.md).

## Goal and division of labor

| Actor | Owns |
|---|---|
| **You** | Policy, repo choice, Hermes config/skills, when to run cycles, human disposition decisions |
| **Hermes** | Intake, scheduling, messaging, deciding *when* to call `codex-team` |
| **`codex-team` (Rust)** | Admission, locks, guards, verification, `state.json`, queue status, and Git mechanics; manual mode requires separate exact human approval |

Do **not** nest Hermes `delegate_task` as a second full orchestrator on top of CEO/PM/Senior/Worker roles. Prefer Hermes as a supervisor that shells out to `codex-team`.

## Prerequisites checklist

1. Hermes Agent installed and working (`hermes --help` or your usual entrypoint).
2. `codex-team` built and on `PATH` (or use an absolute path to `target/release/codex-team`).
3. Codex CLI available (`codex --version`); `codex-team doctor --repo <repo>` should look healthy.
4. Target repo is a Git worktree (default `require_git = true`).
5. Prefer a **disposable throwaway repo** for the first POC, not this orchestrator’s own working tree.

Quick doctor:

```sh
cargo build --release
./target/release/codex-team doctor --repo /path/to/target-repo
```

## Hard rules (non-negotiable)

1. Hermes must **not** write `.orchestrator/state.json`, prompts, schemas, lock files, or create Git commits itself. In manual mode it must also wait for a fresh human decision before calling CLI/MCP pending-commit approval.
2. Always close stdin when Hermes runs `step` non-interactively (`</dev/null`). Non-TTY sessions stop on human-disposition pauses instead of blocking; still treat `outcome: "paused"` as your cue to intervene.
3. One supervisor per repo at a time when cycling; mutating commands take `.orchestrator/codex-team.lock`.
4. Read receipts and `events.jsonl` **after** the process exits. Do not race another `step` against a live lock.

## Phase 0 — Freeze the contract (30 minutes)

Confirm the machine receipts you already have:

```sh
REPO=/path/to/disposable-repo

# One cycle + JSON receipt (stdout is exactly one JSON object when --json)
./target/release/codex-team step --repo "$REPO" --json </dev/null

# Snapshot
./target/release/codex-team status --repo "$REPO" --json

# Observe-only (no queue recover/save)
./target/release/codex-team status --repo "$REPO" --json --observe
```

Or use the checked-in loop: `scripts/hermes_supervisor_loop.sh "$REPO" 5`.

### `step --json` fields (`protocol_version: 1`)

Under `--json`, **stdout is exactly one JSON object**. Progress lines go to stderr.

| Field | Meaning |
|---|---|
| `cycle_id` | Correlates this receipt with a `cycle_receipt` event in `events.jsonl` |
| `outcome` | `continue` · `stopped` · `paused` · `no_work`; `paused` may represent a human-disposition pause or a manual pending commit |
| `mission` | Current mission string |
| `planner_due` / `master_due` | Escalation flags |
| `queue_len` | In-memory queue length after the cycle |
| `human_disposition_required` | `true` if you must answer a pause |
| `pause` | Pause object `{reason, options, source}` or `null` |
| `commit_mode` / `require_clean_start` | Additive supervised-mode configuration snapshot |
| `revision` | Current repository revision used for stale-state preconditions |
| `pending_commit` | Exact pending ID/digest/path/message summary or `null` |
| `pause_fingerprint` | Exact human-pause fingerprint when a disposition pause exists |
| `events_path` | Path to `.orchestrator/events.jsonl` |

Schemas: `schemas/cycle_receipt.schema.json`, `schemas/status_receipt.schema.json`.

### `status --json` fields

| Field | Meaning |
|---|---|
| `project_complete` | Persisted complete flag |
| `queue` | Array of `{task_id, kind, title, status}` |
| `human_disposition_required` | Pause object or null |
| `commit_mode` / `require_clean_start` | Additive supervised-mode configuration snapshot |
| `revision` | Current repository revision for the next exact operation |
| `pending_commit` | Exact manual intent summary or null |
| `pause_fingerprint` | Exact fingerprint required with a guarded disposition |
| `events_path` | Events file |
| `observe` | `true` when `--observe` was set (no queue recover/save) |

Use `status --json --observe` for a read-mostly snapshot (still acquires the repo lock).

### Outcome → your action

| `outcome` | What you / Hermes should do next |
|---|---|
| `continue` | Safe to schedule another `step --json` if you still want work |
| `no_work` | Nothing queued; stop or change mission / backlog yourself |
| `stopped` | Cycle ended without more auto work; inspect status/events before looping |
| `paused` | Inspect both `pause` and `pending_commit`; do **not** blind-loop. Ask the human for disposition or exact commit approval as applicable. |

Answer a pause without TTY:

```sh
./target/release/codex-team disposition --repo "$REPO" --decision preserve --json
# or: recheck | gate | keep-paused
# refresh dirty adoption report only:
./target/release/codex-team disposition --repo "$REPO" --decision keep-paused --refresh-report --json
```

## Phase 1 — Supervisor POC (CLI only; do this first)

### 1. Create a disposable playground

```sh
mkdir -p ~/tmp/codex-team-hermes-poc && cd ~/tmp/codex-team-hermes-poc
git init
echo '# poc' > README.md
git add README.md && git commit -m 'init'

CODEX_TEAM=/path/to/codex_team_orchestrator/target/release/codex-team
$CODEX_TEAM init --repo . --mission "Hermes supervisor POC: tiny bounded tasks only"
```

Edit `NEXT_STEPS.md` in that repo with one tiny approved Worker task (exact allowed files, exact tests). Keep scope mechanical so the POC is about the bridge, not product work.

### 2. Manual loop script (proof before Hermes)

Prefer the checked-in script:

```sh
CODEX_TEAM=./target/release/codex-team scripts/hermes_supervisor_loop.sh "$REPO" 5
```

Equivalent inline loop:

```sh
#!/usr/bin/env bash
set -euo pipefail
REPO="${1:?repo path}"
BIN="${CODEX_TEAM:-codex-team}"
MAX="${2:-5}"

for i in $(seq 1 "$MAX"); do
  echo "=== cycle $i ===" >&2
  receipt="$("$BIN" step --repo "$REPO" --json </dev/null)"
  echo "$receipt"
  outcome="$(printf '%s\n' "$receipt" | python3 -c 'import sys,json; print(json.load(sys.stdin)["outcome"])')"
  case "$outcome" in
    continue) ;;
    paused)
      echo "paused: run disposition then retry" >&2
      exit 2
      ;;
    no_work|stopped)
      echo "done: $outcome" >&2
      exit 0
      ;;
    *)
      echo "unknown outcome: $outcome" >&2
      exit 1
      ;;
  esac
done
```

Run until you trust lock behavior, pauses, and receipts.

### 3. Hermes skill / cron pattern (proven CLI path)

Give Hermes a skill or standing instruction like the checked-in draft [`hermes-skill-draft.md`](hermes-skill-draft.md):

1. Only operate on the approved `$REPO`.
2. Run exactly: `codex-team step --repo $REPO --json` with stdin closed.
3. Parse stdout as the sole JSON receipt; never edit `.orchestrator` or run `git commit`.
4. On `paused`, notify you with `disposition` options; do not invent a decision.
5. On `continue`, optionally call `status --json --observe` once, then decide whether another step is warranted (cap cycles).
6. After exit, skim `events_path` tail for failures; do not treat model prose as authority.

Example Hermes terminal invocation (shape only; adjust to your Hermes tooling):

```sh
codex-team step --repo /Users/you/tmp/codex-team-hermes-poc --json </dev/null
```

Capture stdout as the cycle receipt.

### 4. POC success criteria

- [x] Hermes (or your script) can run ≥3 cycles without touching control-plane files by hand.
  - Evidence (2026-07-14 disposable `~/tmp/codex-team-hermes-poc`): pause receipt + disposition `preserve` + ≥2 later `stopped` cycle receipts (`protocol_version: 1`). Hermes skill `-s codex-team` ran `doctor --json` + `status --json --observe` only.
- [x] `paused` surfaces to you and is cleared via `disposition`, not by editing `state.json`.
  - Dirty-adoption pause during `needs_reconciliation`; cleared with `disposition --decision preserve --json` → `outcome: recorded`, `human_disposition_required: false`.
- [x] Accepted work, if any, shows Rust-owned commits and Done Status in `NEXT_STEPS.md`.
  - **None accepted in this POC.** Real OpenAI `codex` was not on `PATH`. Ollama Worker produced schema-invalid JSON (`cwd: null`) and step exited **without** a cycle receipt — task left `Blocked` for bridge proof only.
- [x] No nested Hermes-of-Hermes / double Worker loops.
  - Supervisor used CLI + skill only; no Hermes `delegate_task`.

#### POC findings (historical MCP prerequisites)

1. ~~Hard `step` failures (schema/network) exit non-zero **without** a cycle receipt~~ — **Fixed in ORCH-HERMES-003:** `outcome: "error"` with additive `error_kind` / `error_summary` (still exit non-zero).
2. Ollama/MLX local roles + structured schemas are brittle — mitigated with `json_hydrate` (null strings / enum maps); still prefer solid models for Worker.
3. Dirty-adoption pause only fires while `needs_reconciliation` is true (pre-existing control docs at `init`).
4. Worker-eligible cards need `Status: Approved` (`Ready` is pending-approval only).
5. Operator machine had intermittent `ollama serve` dropouts on `:11434` during the run; use `[routing].local_provider_failure_budget`.

See also [`HERMES_MLX_DOGFOOD.md`](HERMES_MLX_DOGFOOD.md) for Rust→MLX dogfood results and the ORCH-MLX-WRITE-001 edit-lane decision (Hermes→MLX tools preferred for local file edits; Rust→MLX Worker is chat/JSON only).

## Phase 2 — Stabilize the CLI bridge

Implement only when Phase 1 is boring and reliable:

1. Treat `protocol_version: 1` as frozen; version bumps require a short note in this guide + `HERMES.md`.
2. Use the repo skill draft [`hermes-skill-draft.md`](hermes-skill-draft.md); copy into your Hermes skills dir.
3. ORCH-CA-001 adds a strict task proposal boundary: preview one
   `codex-team.task_import.v1` object, then apply that exact digest and
   repository revision. Do not scrape or rewrite `NEXT_STEPS.md` to import it.
4. Outside that typed import path, **you or Rust admit** into `NEXT_STEPS.md` /
   Planner; Hermes does not smash Status to Done.
5. `cycle_id` + `status --observe` are already available; keep them in skill recipes.

## Phase 3 — MCP implementation preview (after Phase 2)

Hermes natively loads MCP servers from `~/.hermes/config.yaml` under
`mcp_servers` (stdio or HTTP).

ORCH-CA-001 now contains an SDK-backed stdio server in the main binary. It is
not production-approved: generic SDK/fake-process coverage and actual
`ca::McpManager` observe/operate compatibility pass, while the final branch
matrices and supervised clean MCP pilot remain pending. The public `role-run`
contract is implemented in the clean target integration clone; the original
dirty checkout remains read-only.

The server pins one canonical Git root at startup, accepts no repository
override in tool arguments, advertises zero resources, and exposes this exact
surface:

| Access | Tool | Notes |
|---|---|---|
| Observe (default) | `status` | Observe-only typed status for the pinned repo |
| Observe (default) | `doctor` | Typed readiness; unhealthy remains a structured tool error |
| Observe (default) | `step_status` | Read or poll one durable job for 0–20 seconds |
| Operate | `task_import_preview` / `task_import_apply` | Preview is read-only; apply requires the exact digest and revision |
| Operate | `step_start` / `step_cancel` | Start returns immediately with a job ID; poll instead of retrying start |
| Operate | `commit_pending` | Separate exact ID/digest/revision-bound commit approval |
| Operate | `disposition` | Separate decision/revision/pause-fingerprint-bound human disposition |

Operate startup fails unless the repository is initialized with
`commit_mode = "manual"`, `require_clean_start = true`, and only cancellable
Codex/`ca` role providers. Never enable operate merely because tool discovery
succeeds.

Illustrative **observe-only pilot** config after the automated gates pass:

```yaml
mcp_servers:
  codex_team:
    command: "/path/to/codex-team"
    args: ["mcp", "--repo", "/canonical/path/to/disposable-repo", "--access", "observe"]
    timeout: 60
    connect_timeout: 60
    tools:
      include: [status, doctor, step_status]
      prompts: false
      resources: false
```

Until `PLAN.md` records the final matrices and supervised cancellation/pilot
gates as passing, stay on the proven bounded CLI Phase 1 path for
production-shaped supervision.

## Phase 4 — Optional deeper integration

Only after Phase 2 contracts are stable:

- Hermes as a **Worker provider** behind a `CodexRunner`-like trait, **or**
- Hermes as a policy sidecar.

Do not start here. Supervisor + Rust ownership is enough for production-shaped dogfooding.

## Day-to-day operator playbook

1. **Morning:** `status --json` on the supervised repo; glance `human_disposition_required` and queue.
2. **Run:** Hermes or script drives bounded `step --json` loops with a hard cycle cap.
3. **Pause:** You run `disposition`; Hermes waits.
4. **Stuck:** `doctor`, then `OPERATIONS.md` crash/lock section; check `.orchestrator/runs/` and `cost.jsonl` for `no_signal`.
5. **Never:** Let Hermes `git add`/`commit`, edit lock/state, or unlock by deleting the lock while a process is alive.

## Anti-patterns

| Anti-pattern | Why it hurts |
|---|---|
| Hermes edits Worker files outside a task card | Bypasses guard / evidence |
| Hermes and you both running `step` on same repo | Lock races / corrupt mental model |
| Looping forever on `continue` without caps | Token/cost burn |
| Treating Senior/Worker JSON as already-committed truth | Only Rust commits and Done Status are authority |
| Building MCP before CLI POC works | Debugs two systems at once |

## Pointers

- Short contract: [`HERMES.md`](HERMES.md)
- Ops / recovery: [`OPERATIONS.md`](OPERATIONS.md)
- Agent code map: [`AGENT_MAP.md`](AGENT_MAP.md)
- Hermes MCP docs (external): https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp
