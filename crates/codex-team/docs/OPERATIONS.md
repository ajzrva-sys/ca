# Operations Runbook

## Tool Health

`codex-team` records compact tool and Codex-call health under `.orchestrator/observations/tool_health.jsonl`. Watch and cost reports summarize advisory cooldowns in `tool_health.json` and `tool_health.md`.

Cooldowns are advisory. They explain recent failures such as rate limits, invalid models, authentication errors, model unavailability, permission errors, timeouts, or malformed output. They do not suppress role calls until a future Rust-owned fallback policy exists.

## Derived Status

`status` and `watch` render a derived status alongside persisted routing flags. The derived value is an operator triage hint, not a replacement state machine.

`no_signal` means a recent role prompt/run artifact exists, but no matching model-call record, final JSON, or completed stdout item appeared after the configured role timeout plus 60 seconds. Check `.orchestrator/runs/` for the prompt/stdout/stderr files and `.orchestrator/cost.jsonl` for the expected model-call record.

## Route Benchmark

Use:

```sh
codex-team route-bench --repo /path/to/repo
codex-team route-bench --repo /path/to/repo --json
```

The benchmark scores built-in synthetic profiles plus recent approved task profiles against existing `ExecutionLane` values. It writes `.orchestrator/observations/route_benchmark.json` and `.orchestrator/observations/route_benchmark.md`. It does not call Codex or execute commands.

## Characterization Contracts

Before launching Worker, Rust derives an acceptance contract from the task card: definition of done, verification commands, review tier, automation level, allowed/read-only files, and stop conditions.

Risky migration, refactor, port, rewrite, or parity tasks that touch source or broad file sets must include characterization evidence such as golden, snapshot, parity, baseline, fixture, or characterization verification. Missing evidence blocks Worker with `missing_characterization_contract` and routes back to Project Manager.

## Crash / Lock Recovery

- Concurrent mutating commands acquire `.orchestrator/codex-team.lock`. Dead PID locks are recoverable; live/corrupt locks fail closed.
- If a cycle crashes mid-acceptance, queue Done/`Completed Evidence` may need reconciliation against git history: failed commits should roll back provisional Done notes (ORCH-INTEGRITY-001).
- Orphaned Worker dirty trees: inspect `git status` and guard changed-file reports before re-running `step`.
- Corrupt `state.json`: restore from the latest known-good state or re-init routing flags carefully; do not invent queue Status values.
- Prefer `codex-team doctor` then `codex-team status --json` for triage.

## Hermes Supervisor

See `HERMES.md` (triad + contract + edit routing), `HERMES_IMPLEMENTATION_GUIDE.md`, and living skills under `~/.hermes/skills/codex-team*`. Hermes may drive `step`/`status`/`disposition`/`doctor`/`providers --json` with stdin closed; use `status --observe` for read-mostly snapshots. On the **Rust lane**, Rust owns admit/guard/verify/commit. Optional Ollama / MLX LM role calls are Rust-owned chat/JSON (`roles.*.provider`); they do **not** write the worktree — prefer Hermes→MLX tools for local AI edits (ORCH-MLX-WRITE-001). Do not `delegate_task` as Worker/PM/Senior for Rust-lane admit/verify/commit.
