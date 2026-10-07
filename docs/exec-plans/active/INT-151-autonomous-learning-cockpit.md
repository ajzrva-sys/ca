# INT-151 — Autonomous Learning and Delegation Cockpit

Status: ACTIVE
Created: 2026-08-21
Last updated: 2026-08-23

## Purpose

Expose the orchestrator-owned autonomous learning state and controls through
`ca` without granting a model authority to enroll, pause/resume, roll back,
approve deployments, or change provider credentials. Extend per-role
delegation settings with `same`, `fixed`, and `learned` while preserving `same`
as the default.

## Safety boundary

- All host controls call typed MCP tools directly; they never create a model
  request.
- Do not install binaries, alter a real profile, access credentials, or invoke a
  real provider during implementation or validation.
- `oracles/` and `reference/` remain read-only.
- Learned routing is contained to configured, healthy, explicitly allowed
  routes and cannot fall back after a request starts.
- Distill output becomes immutable candidate input; it cannot directly widen
  runtime authority.

## Milestones

- [ ] Add backward-compatible per-role same/fixed/learned delegation settings
      and configured provider/model allowlists.
- [x] Add model-free Plan/Settings views for status, objectives, traces,
      experiments, active revisions, A/B slots, usage, pause/resume, and exact
      rollback.
- [x] Render learning and deployment events in the orchestration timeline with
      bounded detail and copy feedback.
- [x] Route Dream/Distill skill output through immutable candidates and record
      activation/result revisions.
- [x] Add narrow/wide render snapshots and tests proving host controls make no
      provider request.
- [x] Run focused and repository completion checks using disposable state.

## Progress and remaining work

- Global child delegation now supports `same`, exact `fixed`, and explicitly
  allowlisted `learned` modes. Existing route-only profiles migrate as fixed.
- The host-rendered cockpit reads status, objectives, evaluations, and
  deployments and performs revision-bound pause/resume/rollback calls directly.
- The full all-target/all-feature test, strict Clippy, formatting, release, and
  help/version gates pass without provider traffic or profile mutation.
- Explicitly enrolled profiles now queue sanitized, digest-bound Dream/Distill
  intents through an offline export/completion bridge. LRN-166 removed the
  import-time acknowledgement gap: terminal replay rejection or successful
  journaled memory/inert-skill publication now precedes acknowledgement, and
  duplicate completion is idempotent. Unenrolled profiles retain legacy
  behavior.
- The cockpit now renders bridge/profile health, queued intents, workflow stage,
  candidate author/type, judges, replay progress, decisions, failures,
  cooldowns, learned skill/recipe revisions, and avoided calls. The refreshed
  full matrix and disposable process-level fake-provider activation pilot pass.
- Real release `ca role-run` compatibility passes. Actual `ca::McpManager`
  observe/operate compatibility passes against the final debug orchestrator;
  the release orchestrator correctly refuses operate mode because production
  Seatbelt cannot self-test inside this managed app environment.
- The learning portion is implementation-complete and the combined cockpit
  status exposes service health, queue/workflow state, token/call usage,
  promotions, rollbacks, and failures. It remains operationally pending until
  LRN-166's exact LaunchAgent activation and one live terminal workflow pass.
- Per-orchestrator-role settings remain a separate follow-on; ordinary `ca`
  child delegation continues using the existing global same/fixed/learned
  policy.

## Allowed implementation paths

- `src/{config,main,learning}.rs`
- `src/agent/subagent.rs`
- `src/tui/**`
- `docs/SELF_IMPROVEMENT.md`
- `docs/exec-plans/active/INT-151-autonomous-learning-cockpit.md`
- `CONTINUITY.md`
- `NEXT_STEPS.md`
- focused tests adjacent to these modules
