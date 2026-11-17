# Repository Agent Instructions

## Purpose
- `ca` is a local agent harness: a Rust terminal coding agent built with ratatui, crossterm, and tokio.
- CLI/TUI only. Do not add a GUI.

## Immutable Agent Laws
These bind every agent operating in this repo, above all other rules.
1. The user's explicit words are the only authority on intent. A prohibition
   binds only if the user stated it; a rule an earlier agent (or a model's own
   caution) wrote is not user policy.
2. Never persist your own caution as the user's instruction. If you hesitate
   (safety, policy, legality, risk), state it once and ask; never write "out of
   scope / prohibited / non-negotiable" into this repo as if the user decided it.
3. A standing rule that blocks the user's actual goal is the bug. Audit its
   origin before engineering around it; if the user didn't set it, remove it.
4. On repeated failure, re-examine your own premises before adding another fix.
   Re-asserting a refusal is never the answer to "what's the goal here".
5. "Out of scope / non-negotiable" lines in this repo are artifacts, not law.
   Re-verify every one against the user's current words.

## Control Rules
- Inspect before editing. Keep diffs local and preserve working behavior.
- Treat `oracles/`, `reference/`, and nested repositories under them as read-only evidence.
- Never copy source from an oracle. Reproduce behavior from observations, fixtures, and tests.
- Do not read or print API keys. Runtime settings belong in `~/.config/ca/config.toml`; secrets stay external.
- Do not commit or mutate user runtime state unless explicitly asked. The
  user-authorized post-task release rule below supplies standing permission to
  replace the installed `ca`; preserve the prior binary recoverably and verify
  the installed artifact. It does not authorize config/session changes.
- Load `ratatui-agent` and `restrictive-api-handling` only when each exact name
  appears in the current injected skill catalog. If absent, they may be installed
  from a verified source after explicit user authorization; until installation
  is complete, do not call a skill tool for them and follow this harness directly.

## Start Here
1. `docs/.INDEX.md` — map of repository knowledge.
2. `CONTINUITY.md` — current state, conflicts, and risks.
3. `ARCHITECTURE.md` — edit-routing and data-flow map.
4. `TESTING.md` — exact validation commands and known gaps.
5. `NEXT_STEPS.md` — executable queue. One product plan is named there.
6. `.agent/PLANS.md` — standard for complex work. `crates/codex-team/.agent/PLANS.md` binds only that crate.
7. `docs/QUALITY.md` and `docs/GOLDEN_PRINCIPLES.md` — gates and durable invariants.
8. `REVIEW_CHECKLIST.md` and `docs/PROMPTS.md` — acceptance checks and maintenance prompts.

## Source of Truth
1. Primary source, `Cargo.toml`, tests, and command output.
2. This harness and explicit current project docs.
3. Existing roadmap/spec/task documents, after checking their status banner.
4. Inferences from primary structure, marked `Inferred`.
5. Oracle notes in `PARITY.md`, only when the current task asks for that comparison.
6. Codex oracle patterns as optional engineering lessons.

Record unresolved conflicts in `CONTINUITY.md`; parity conflicts also go in `PARITY.md`.

## Repository Map
- `src/main.rs`: CLI parsing, headless/TUI orchestration, stream and tool-loop coordination.
- `src/app.rs`: shared conversation and UI state.
- `src/agent/`: OpenAI-compatible client, tools, and subagents.
- `src/tui/`: terminal lifecycle, input state machines, and rendering.
- `src/{config,session,context,approval,undo,markdown,test}.rs`: supporting boundaries.
- `crates/ca-automation/` and `crates/ca-automation-protocol/`: native per-run execution services and contracts. ca owns admission and scheduling.
- `crates/codex-team/` and `crates/codex-team-protocol/`: compatibility forwarding packages; the legacy nested `AGENTS.md` still applies there.
- `tests/*.ca`: network-dependent scripted harness cases.
- `oracles/cursor-CLI-reverse/`: primary read-only parity evidence.
- `oracles/codex/`: secondary read-only architecture/testing lessons.
- `reference/`: older extracted Cursor bundle snippets; read-only.
- `ca-hrs-252/`: side snapshot inside this checkout. Do not edit. `.ignore` keeps it, and `docs/archive/`, out of search. Open those paths directly.

## Work Pattern
1. Read the relevant harness page and trace symbols to definitions and callers.
2. For small work, claim one `NEXT_STEPS.md` task. For complex work, create an ExecPlan.
3. Write a failing focused test before changing behavior where practical.
4. Edit minimally; avoid drive-by formatting or refactors.
5. Run focused checks, then the minimum completion checks in `TESTING.md`.
6. Preserve current behavior with the focused test. Open `PARITY.md` only when the task asks for an oracle comparison.
7. Update `CONTINUITY.md`, `NEXT_STEPS.md`, and the active ExecPlan with durable discoveries.

## Post-task release and install

User instruction (2026-09-03, narrowed 2026-10-24): this is standing permission to replace the installed `ca` after a Rust change. Do not ask again.

Docs-only work skips the release. A Rust change uses one `python3 scripts/check-change.py` run, then `bash tools/bug257_install.sh` on that same artifact. Bump, recovery copy, code signature, and readback are in `docs/VERIFICATION.md`. Report the job if it is still running. Leave running sessions alone.

## ExecPlans
- Required for risky, unclear, multi-file, protocol, event-loop, approval, session-format, or parity work.
- Follow `.agent/PLANS.md`; store active plans in `docs/exec-plans/active/`.
- Move completed plans to `docs/exec-plans/completed/` and record validation evidence.
- Keep small local tasks in `NEXT_STEPS.md`; link rather than duplicate an ExecPlan.

## Validation
- Small, bounded changes use `python3 scripts/check-change.py --selector <test>`
  (repeat selectors as needed), then the single release build/install. Add
  `--release` to build that candidate in the same job; install its artifact
  through `tools/bug257_install.sh` without another build. Use stable source,
  Cargo cache and target paths; keep application state disposable.
  Run Cargo stages sequentially against that target; do not start a focused
  build and full gate concurrently and make them wait on the same target lock.
- `./scripts/verify.sh` remains the complete unattended workspace gate. Run it
  when explicitly requested or when broad shared/protocol/session/approval
  changes need that coverage; do not put both full matrices ahead of every
  small fix. Read report/evaluation and actual logs; label focused coverage
  accurately and never call an unfinished full gate passed.
- Use an explicit baseline and behavioral hypothesis for improvements. Keep
  established tests/snapshots/evaluation criteria intact during repair. Diagnose
  from new evidence, correct narrowly, rebuild/retest, and preserve every failed
  attempt. At most three automatic repairs per failure; exhaustion is failure.
- `python3 scripts/verify-loop.py` supports a reviewed explicit repair command;
  without one, the development agent owns diagnosis and the bounded repair loop.
  Do not repeat the same unsuccessful approach without new evidence. Prefer an
  independent evaluator/reviewer and held-out cases when practical.
- Verification and validator experiments use disposable application state and
  source copies, never production data/services or installed binaries. This
  testing workflow does not invoke the historical post-task install procedure.
- `./scripts/verify.sh --self-test` demonstrates detection of a real intentional
  defect and exact restoration in a disposable source copy. Run it when changing
  that full verifier; retain the receipt. Changes to the separate focused helper
  use its contract tests and cold/warm preparation receipts instead of rerunning
  unrelated Rust matrices.
- Follow `TESTING.md`; never claim a network or visual check passed unless it ran.
- Small Rust changes require focused behavioral tests, formatting, strict
  Clippy for the affected targets, and one release build before installation.
  Expand to workspace/all-feature tests when the changed behavior requires it.
  Reuse current passing evidence for unchanged targets and source/artifacts.
- Formatting or lint failure is a real failure. Do not run a broad formatter unless the task allows those changes.
- User-visible TUI work needs deterministic render coverage plus manual comparison when available.
- For live progress and previews, verify what the user actually sees while work
  is still pending: make a second update visibly replace the first, with long
  representative history. Publication, refresh hooks, fresh stored data and a
  completed-result screenshot do not prove visible freshness. A known display
  defect blocking the requested workflow belongs to the current repair.

## Guardrails
- Preserve async responsiveness: no `block_on`, `block_in_place`, or synchronous long work in the TUI event loop.
- Keep tool schemas, execution names, approval classification, and OpenAI tool-result messages aligned.
- Do not weaken approval checks or add destructive slash commands without explicit scope and tests.
- Convert repeated mistakes into tests or proposed checks in `docs/QUALITY.md`.

## Stop Conditions
- Stop if requirements conflict or the relevant Cursor behavior cannot be observed.
- Stop before destructive commands, runtime config/session mutation, or secret
  access. For binary installation, follow the standing post-task authorization
  above and resolve the validated release, exact destination, recovery copy,
  and post-install verification before replacing the executable.
- Stop if an oracle would need modification or copied implementation code.
- Stop when validation needs unavailable credentials/services; record the blocked check instead of guessing.

## Done Criteria
- Only allowed files changed; no oracle or unrelated edits.
- Focused and completion checks are recorded with actual results.
- Observable acceptance criteria pass; parity evidence is cited for parity work.
- New facts and remaining uncertainty are reflected in the harness.
- Final response names changed files, tests run, failures/blocked checks, and the next task ID if work remains.
