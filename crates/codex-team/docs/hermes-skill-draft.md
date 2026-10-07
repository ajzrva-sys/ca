# Hermes skill draft (mirror)

**Installable source of truth:** Hermes owns living skills at `~/.hermes/skills/codex-team/` (in-repo contract exports deleted 2026-07-20).
**Install:** `scripts/install_hermes_skill.sh` → `~/.hermes/skills/codex-team/`
**Checklist:** [`HERMES_INTEGRATION_CHECKLIST.md`](HERMES_INTEGRATION_CHECKLIST.md)

Standing instructions for **Nous Hermes Agent** supervising a `codex-team` repo.
Rust owns admission, locks, guards, verification, `.orchestrator/state.json`, and commits.
Hermes only schedules CLI calls and notifies a human on pauses.

## Hard rules

1. Operate only on the **approved** `$REPO` path.
2. Never write `.orchestrator/state.json`, prompts, schemas, lock files, or run `git add` / `git commit`.
3. Always close stdin for non-interactive `step`: `codex-team step --repo "$REPO" --json </dev/null`
4. Treat **stdout as exactly one JSON object** under `--json`. Do not scrape `[Team]` lines (those go to stderr).
5. Cap automatic cycles (suggested default: 5). Never infinite-loop on `continue`.
6. On `outcome: "paused"`, notify the human with disposition options; **do not invent** a decision.
7. One supervisor per repo at a time. Do not start another `step` while a lock is held / process is alive.
8. After exit, skim `events_path` tail for failures; model prose is not authority.

## CLI recipes

```sh
# Preflight (optional)
codex-team doctor --repo "$REPO" --json

# Observe without queue recover/save (still acquires lock)
codex-team status --repo "$REPO" --json --observe

# One cycle
codex-team step --repo "$REPO" --json </dev/null

# Clear a pause only after human instruction
codex-team disposition --repo "$REPO" --decision preserve|recheck|gate|keep-paused --json
```

## Step receipt (`protocol_version: 1`)

Fields include: `cycle_id`, `outcome`, `mission`, `planner_due`, `master_due`, `queue_len`,
`human_disposition_required` (bool), `pause` (object or null), `events_path`.

| `outcome` | Action |
|---|---|
| `continue` | Optional another capped `step --json` |
| `no_work` | Stop or change backlog/mission via human |
| `stopped` | Inspect `status --json` / events before looping |
| `paused` | Notify human; run `disposition` only after their choice |

## Anti-patterns

- Nesting Hermes `delegate_task` as a second Worker/CEO loop for Rust-lane admit/verify/commit
- Expecting Rust→MLX Worker (`provider=mlx_lm`) to write files — chat/JSON only; Prefer Hermes→MLX tools for edits (see `docs/HERMES.md` § Edit routing)
- Editing `NEXT_STEPS.md` Status to Done by hand
- Blind-looping `step` after `paused`
- Deleting the lock while a process is alive

## Pointers

- Contract: `docs/HERMES.md` (triad + edit routing)
- Contract + bridge: `docs/HERMES.md` (merged from former `HERMES_BOSS_BRIDGE.md`)
- Dogfood / write-lane: `docs/HERMES_MLX_DOGFOOD.md`
- Operator guide: `docs/HERMES_IMPLEMENTATION_GUIDE.md`
- Machine accomplishments catalog: `docs/MACHINE_ACCOMPLISHMENTS.yaml`
- Loop script: `scripts/hermes_supervisor_loop.sh`
- Living skills (Hermes-owned): `~/.hermes/skills/codex-team*`
