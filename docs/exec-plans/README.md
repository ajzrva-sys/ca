# Execution Plans

Use an ExecPlan when the work is risky, unclear, multi-file, or likely to span sessions. That includes protocol, event-loop, approval, session-format, and parity changes. The required sections are in `.agent/PLANS.md`.

## Next plan

[AGT-271](active/AGT-271-task-first-economic-delegation.md) is the plan that just finished M5. A live cheaper-quality claim stays separately authorized. [NEXT_STEPS.md](../../NEXT_STEPS.md) points here.

Do not start another file in `active/` unless the task names that plan. Each of those files starts with that sentence. The status line under it is `HELD`. The rest of the plan text was not rewritten.

| Plan | Why it is not the continuation |
|---|---|
| [AGT-269](active/AGT-269-proactive-team-continuous-learning.md) | HELD. Not the continuation. |
| [AUT-231](active/AUT-231-codex-style-local-automations.md) | HELD. Not the continuation. |
| [BUG-257](active/BUG-257-long-session-responsiveness.md) | HELD. Not the continuation. |
| [BUG-provider-timeout-recovery](active/BUG-provider-timeout-recovery.md) | HELD. October 11 transport recovery. No status banner. |
| [install-binary-tool](active/install-binary-tool.md) | HELD. Design for an `install_binary` tool. Not the continuation. |
| [INT-151](active/INT-151-autonomous-learning-cockpit.md) | HELD. Not the continuation. |
| [LRN-150](active/LRN-150-reviewed-codex-archive-learning.md) | HELD. Not the continuation. |
| [ORC-258 oracle improvements](active/ORC-258-oracle-informed-improvements.md) | HELD. Not the continuation. |
| [ORC-258 recovery evidence](active/ORC-258-recovery-automation-evidence.md) | HELD. Not the continuation. |
| [ORC-291](active/ORC-291-siloed-lane-orchestration.md) | HELD. Every milestone in Progress is checked, including install as 0.1.26. |
| [REL-156](active/REL-156-reproducible-release-baseline.md) | HELD. Not the continuation. |
| [SEC-158](active/SEC-158-privilege-supervisor-and-service-v2.md) | HELD. Not the continuation. |

Not a task to start:

- [LRN-166](active/LRN-166-autonomous-learning-loop-completion.md) stays in `active/` because background activation was deferred by the user's manual-learning decision.
- [ORC-258 baseline exceptions](active/ORC-258-baseline-test-exceptions.md) records a 2026-10-05 red run. The index says AUT-259 later passed that workspace. Do not treat the file as new work.
- `AUT-231-resume/`, `AUT-231-dirty-baseline.status.gz.b64`, and `CUR-204-dirty-baseline.status.gz.b64` are stored next to the plans. They are not plans.

Filed under `completed/` on 2026-11-13 because the plan's own status said installed, stopped, superseded, or no further work. The text was not edited:

- `ISO-337-execution-isolation.md` — installed as ca 0.1.95
- `FLT-342-filter-proof.md` — installed as ca 0.1.100
- `CHG-343-chatgpt-plan-siwc.md` — installed as ca 0.1.109
- `FLT-328-desktop-host-runtime-integration.md` — stopped
- `FLT-326-same-client-cyber-comparison.md` — no further experiments queued
- `FLT-327-native-startup-repair-and-faithful-replay.md` — no further runs queued
- `LRN-170-one-shot-learning-pass.md` — superseded by LRN-171

Small local work stays in `NEXT_STEPS.md`. Link the plan from that queue. One description lives in the plan.

## Where plans live

- In progress: `active/<ID>-<slug>.md`
- Accepted: `completed/`
- `crates/codex-team/docs/exec-plans/` belongs to that crate and follows `crates/codex-team/.agent/PLANS.md`.

Move a plan to `completed/` only after its acceptance criteria and the commands that ran are written in the plan. Update `CONTINUITY.md` and the `NEXT_STEPS.md` pointer in the same pass. Leave failed attempts in the plan.

Do not copy historical `.hermes/plans/` here. Copy still-valid evidence into a new plan.

## Validation evidence

Record the command, the receipt under `scratch/`, and whether `./scripts/verify.sh` ran. A focused `python3 scripts/check-change.py` receipt covers that selector. It does not certify the workspace. Docs-only harness edits do not bump or install `ca`.
