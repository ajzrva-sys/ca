Moved from crates/codex-team/NEXT_STEPS.md. This body is not the queue. The root continuation is AGT-271.

# NEXT_STEPS

Use this file for small, local, executable work. Use `docs/exec-plans/active/` for complex refactors or risky multi-step changes; link the plan here instead of copying it.

Completed tasks have been moved to [`docs/COMPLETED.md`](docs/COMPLETED.md).

## ORCH-SELF-IMPROVE-002: Autonomous Learning Fabric

Status: Implemented — isolation-capable host confirmation pending
Priority: High

Plan: [`docs/exec-plans/active/orch-self-improve-002.md`](docs/exec-plans/active/orch-self-improve-002.md)

Next acceptance slice:
- Rerun release `ca::McpManager` operate compatibility on a host where the
  production macOS verification-isolation backend can self-test.
- Only after a separate exact enrollment approval, perform a real-profile pilot
  with explicitly configured routes; live provider traffic remains separately
  authorized.
- Keep source transitions, builds, commits, candidate binary activation,
  deployment, service installation, and live provider traffic inactive.

## ORCH-CA-001: Production-Safe `codex-team` and `ca` Integration

Status: Active
Priority: High

Plan: [`PLAN.md`](PLAN.md)

Allowed files:
- `PLAN.md`
- `Cargo.toml`
- `Cargo.lock`
- `src/`
- `schemas/`
- `tests/`
- `skills/`
- `scripts/`
- `README.md`
- `ARCHITECTURE.md`
- `CONTINUITY.md`
- `TESTING.md`
- `NEXT_STEPS.md`
- `docs/`

Stop conditions:
- The separate `ca-tui-polish` checkout was retired by root `CON-153`; work now
  occurs only under root active ExecPlans in `/Users/aji/project/ca`.
- Preserve every unrelated dirty byte. Do not reset, clean, stash, or infer
  commit authorization from implementation work.
- Do not touch `oracles/`, `reference/`, credentials, sessions, installed binaries, or real runtime configuration.
- Do not commit, merge, install, run live-provider dogfood, or submit
  disposition without separate current authority.

## ORCH-LEARN-COMPILER-001: Controlled Learning Compiler v1

Status: Active
Priority: High

Plan: [`docs/exec-plans/active/orch-learn-compiler-001.md`](docs/exec-plans/active/orch-learn-compiler-001.md)

Allowed files:
- `Cargo.toml`
- `Cargo.lock`
- `src/`
- `schemas/`
- `tests/`
- `README.md`
- `ARCHITECTURE.md`
- `CONTINUITY.md`
- `TESTING.md`
- `NEXT_STEPS.md`
- `docs/`

Stop conditions:
- Learned recipes remain evidence-only and may never edit product files.
- Approval and execution fail closed without healthy verification isolation.
- Do not read credentials, persist raw model/tool output, or use network access.
- Do not install binaries, mutate real profiles, or run live-provider traffic.

## ORCH-ROLE-COMMIT-002: Evaluate Hunk-Scoped Role-Authorized Commits

Status: Proposed
Priority: Medium

Allowed files:
- `src/app.rs`
- `src/app_tests.rs`
- `NEXT_STEPS.md`
- `CONTINUITY.md`
- `TESTING.md`

Context:
- ORCH-ROLE-COMMIT-001 is path-scoped: Rust stages only explicitly allowed selected paths, but it does not split hunks inside one file.
- Current recovery wants independent ORCH-LEARN and CHURN slices; mixed-file dirty state can still require manual reconciliation.

Acceptance criteria:
- Decide whether to support hunk-scoped commit requests or to emit a clear stop condition when an allowed path appears to contain mixed unrelated edits.
- Add focused tests for whichever policy is selected.
