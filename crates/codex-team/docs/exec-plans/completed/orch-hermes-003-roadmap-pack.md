# ORCH-HERMES-003 + Roadmap P0–P2 (Milestone pack)

## Purpose / Big Picture

Make Hermes+MLX operable: always emit `--json` cycle receipts on hard failures; hydrate brittle local-provider JSON; stop empty-queue Planner churn; doctor MLX model presence; record supervisor episodes + advisory provider hints; dogfood notes.

## Progress

- [x] Failure receipts on `step --json`
- [x] Local JSON hydrate + doctor MLX model checks
- [x] Empty-queue deterministic stop
- [x] Supervisor episodes + routing hint / providers JSON
- [x] MLX dogfood notes + docs/skill updates
- [x] Validation

## Decision Log

- Add `outcome: "error"` additive fields on cycle receipt (protocol_version stays 1).
- Hydrate null stringish fields before role JSON deserialize; map known bad enums.
- Empty queue + no escalation flags + no unfinished/approved active work → `stopped` without Planner/CEO.
- Supervisor episodes written by Rust on every cycle receipt (not Hermes-owned).
- `[routing].allow_provider_override` defaults false; in-memory worker override only when true.

## Validation

- `cargo test hermes_` — 9 passed
- `cargo test hydrate` — 2 passed
- `cargo test` — 290 passed
- `cargo fmt -- --check` — passed
- Smoke: `step --json` on missing repo → `outcome: error` on stdout, exit 1

## Outcomes & Retrospective

P0–P2 roadmap items shipped in one pack. Live MLX Worker dogfood still needs a running `mlx_lm.server` with `/v1/models` catalog; plumbing and docs ready.
