# ORCH-HERMES-002: Hermes Supervisor Ready Pack

## Purpose / Big Picture

Close the gaps Hermes itself listed for day-to-day supervision of `codex-team`: installable skill packaging, an explicit CLI exit-code contract, a copy-paste supervisor setup block, a checklist mapping Hermes asks → repo truth, and a working local Hermes `custom:ollama` provider config.

## Progress

- [x] ExecPlan + NEXT_STEPS task
- [x] `skills/codex-team/SKILL.md` + install script
- [x] Exit-code contract docs + doctor `--json` exit 1 when unhealthy + check `severity`
- [x] `docs/HERMES.md` setup block + `docs/HERMES_INTEGRATION_CHECKLIST.md`
- [x] Fix `~/.hermes/config.yaml` local Ollama named custom provider
- [x] Validation + CONTINUITY / MACHINE catalog / plan complete

## Surprises & Discoveries

- Hermes `model.provider: custom:ollama` fails until a matching `custom_providers` entry named `ollama` exists; FAQ also allows bare `provider: custom` + `base_url: http://localhost:11434/v1`.
- Config previously pointed `base_url` at DeepSeek while claiming `custom:ollama`.
- Cycle outcomes stay in JSON; only doctor unhealthy maps to exit `1` after emitting JSON.

## Decision Log

- Keep cycle `outcome` in JSON (not exit codes); supervisor script maps `paused` → exit 2.
- `doctor --json` emits receipt then exits `1` when `healthy: false`.
- Skill lives in-repo at `skills/codex-team/`; install copies into `~/.hermes/skills/codex-team/`.
- Named provider `custom:ollama` kept; added `custom_providers[name=ollama]` + local `base_url`.

## Context and Orientation

- Receipts: `src/app/receipts.rs`, schemas under `schemas/*_receipt.schema.json`
- Docs: `docs/HERMES.md`, `docs/HERMES_INTEGRATION_CHECKLIST.md`, `skills/codex-team/SKILL.md`
- Hermes home: `~/.hermes/config.yaml` (operator local; not committed)

## Plan of Work

Completed as implemented in the working tree.

## Concrete Steps

Completed.

## Validation and Acceptance

- `cargo test hermes_` — 7 passed
- `cargo test` — 286 passed
- `cargo fmt -- --check` — passed
- `scripts/install_hermes_skill.sh` — installed; `hermes skills list` shows `codex-team`
- `hermes doctor` — `custom:ollama` no longer unknown
- `hermes chat -Q --max-turns 1` — replied `hermes-ollama-ok` on default provider
- Uninitialized `doctor --json` — exit 1 with `healthy: false` + `severity: error` on failing checks

## Idempotence and Recovery

- Re-run install script with overwrite.
- Restore `~/.hermes/config.yaml` from `.bak.orch-hermes-002-*` if needed.

## Artifacts and Notes

- Skill at `~/.hermes/skills/codex-team/`
- Config backup(s) under `~/.hermes/config.yaml.bak*`

## Interfaces and Dependencies

- CLI exit codes, doctor receipt schema (`severity`), Hermes skill name `codex-team`

## Outcomes & Retrospective

Hermes P0 packaging/exit/setup needs are met in-repo; local Ollama chat works again via named custom provider. MCP and universal `--json` remain deferred per checklist.
