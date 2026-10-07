# ORCH-SELF-IMPROVE-002 — Autonomous Learning Fabric

Status: ACTIVE
Owner: codex-team
Created: 2026-08-21

## Purpose

Add a disabled-by-default, machine-local self-improvement boundary around the
existing recipe compiler and role runners. The boundary owns strict learning
records, structural secret exclusion, immutable artifact/evaluation records,
operator pause and rollback, and independently activated A/B component slots.
The stable supervisor and safety-kernel manifest are never candidate inputs.

## Safety boundary

- Do not install a login service or switch a real binary while developing or
  testing this plan.
- Do not read provider credentials, real provider configuration, sessions, or
  Keychain values.
- Do not modify `oracles/`, `reference/`, the supervisor after enrollment, or
  existing verification/guard authority.
- All tests use temporary homes, repositories, slots, and fake binaries.
- No autonomous dependency or lockfile changes, live provider traffic, stash,
  reset, clean, force checkout, history rewrite, or conflict resolution.
- Existing behavior is unchanged until exact-digest enrollment.

## Milestones

- [x] Add deny-unknown-field contracts for traces, capsules, objectives,
      artifact revisions, overlays, evaluations, deployments, and regressions.
- [x] Add a private, no-follow, atomic vault with structural trace scrubbing,
      low-disk fail-closed status, immutable records, and tamper checks.
- [x] Add enrollment/status/doctor/pause/resume/rollback/objective/trace/
      deployment CLI contracts with exact digest and stale revision checks.
- [x] Add a separately built stable supervisor with serialized state, A/B
      component slots, compatible known-good pair, health checks, and rollback.
- [x] Add frozen deterministic evaluation and correctness-first promotion
      primitives shared by learned recipes, skills, prompts, and routes.
- [x] Expose read and operator controls through pinned-repository MCP.
- [x] Add trace capture at provider-neutral role boundaries without capturing
      secrets or unknown tool arguments.
- [x] Add exact enrolled `ca` evidence/execution profile contracts, offline
      export/acknowledgement, and enrollment-v1 empty-allowlist migration.
- [x] Add the one-stage active-binary artifact worker, foreground yielding,
      single-worker locking, frozen replay, candidate generation, independent
      judgment, immutable promotion, quarantine, and cooldown recovery.
- [x] Keep source transitions, builds, commits, candidate binary activation,
      and deployment unreachable from the artifact worker.
- [x] Complete focused, all-target, strict lint, release, privacy, cancellation,
      real-binary role compatibility, debug-isolated MCP compatibility, and a
      disposable process-level Codex/DeepSeek/MiMo activation pilot.

## Allowed implementation paths

- `Cargo.toml`
- `src/bin/codex-team-supervisor.rs`
- `src/{lib,cli,config,app}.rs`
- `src/self_improvement/**`
- `src/mcp/**`
- `schemas/self_improvement/**`
- `docs/SELF_IMPROVEMENT.md`
- `docs/exec-plans/active/orch-self-improve-002.md`
- `CONTINUITY.md`
- `NEXT_STEPS.md`
- focused tests adjacent to these modules

## Acceptance evidence

Exact-digest enrollment is idempotent; stale confirmations mutate nothing;
private vault records reject symlinks, traversal, permissive modes, corruption,
and digest mismatch; secret canaries do not survive trace capture; evaluation
plans cannot change after candidates exist; deterministic baseline passes cannot
regress; human evidence outranks all other signals; rollback never invokes a
model; and no default configuration starts learning or deploys a binary.

## Progress and discoveries

- The controlled recipe compiler already supplies hardened machine storage,
  isolated evidence-only execution, activation drift, quarantine, and global
  retirement. The new fabric composes with it instead of replacing its safety
  authority.
- The `ca` updater is an explicit, signed single-component update path. The new
  supervisor must be independent and must retain a verified compatible pair.
- Promoted shared-core packages and provider/model overlays now have an exact
  evaluation-bound path into future role prompts. Multi-file package assets and
  scripts remain inert; only reviewed UTF-8 members are injectable.
- Clean current-branch source fast-forward is implemented as a separate exact
  transition primitive. It rejects stale/dirty state, protected trees,
  dependency manifests, safety-kernel paths, submodules, and multi-commit
  candidates. The supervisor does not invoke it yet.
- Enrolled `ca` Dream/Distill now queues sanitized candidate intents; unenrolled
  profiles retain legacy behavior. The worker covers memory, multi-file skills,
  shared role prompts, target-only provider overlays, and controlled recipe
  approval for admitted evidence-only tasks.
- The refreshed matrix passes 539 library tests (one documented ignore), ten
  integration tests, strict all-feature Clippy, all-feature release, formatting,
  diff, snapshot, and protected-tree gates. The real release `ca` role contract
  passes, and the process pilot exports/acknowledges an enrolled intent, invokes
  fake Codex/DeepSeek/MiMo routes, promotes one skill, and leaves both source
  HEADs and deployment records unchanged.
- Actual `ca::McpManager` observe/operate compatibility passes against the final
  debug binary with the debug-only isolation double. The release server
  correctly fails closed because macOS Seatbelt cannot self-test from this
  managed app environment; rerun that one operate-mode gate on a host where the
  production isolation backend is available before real enrollment.
