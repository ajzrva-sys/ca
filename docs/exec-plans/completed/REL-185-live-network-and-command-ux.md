# REL-185 — Live Network and Command UX Repair

Status: COMPLETE
Owner/session: Codex, 2026-08-26
Last updated: 2026-08-26

## Goal

Make the reported resumed-task workflow behave predictably: interactive command
approval follows the Hermes/MiMo model instead of adding a second network-only
gate, approval prompts show the complete command, absolute paths in prose stay
ordinary composer text, and a bounded `await_job` poll reports a still-running
job as running rather than failed.

## Non-Goals

- Installation was excluded from the implementation plan; a later explicit
  user request authorized installing the already-validated release and is
  recorded in the completion section.
- Do not change or terminate the live remote copy job.
- Do not relax persistent Ask/Deny rules, typed outbound-email approval,
  privilege approval, MCP policy, or command approval outside the selected
  AutoReview/RunEverything mode.
- Do not modify `oracles/` or `reference/`.

## Evidence and Current Behavior

- The supplied approval screenshots show `run_command` argument JSON clipped
  with an ellipsis. `src/tui/render.rs::conversation_lines` explicitly calls
  `truncate(&pa.tool_args, 80)` even though the transcript paragraph can wrap.
- The supplied composer screenshot shows prose ending in
  `/bigmac/sed-ro/seedbox/` becoming `Unknown command`. In
  `src/tui/input.rs`, `refresh_inline_completion` opens the slash palette for
  any active `/` token, while `handle_slash_palette` owns Enter and emits that
  error.
- The resumed session `ses_6J9waKgnriP6heeUNe7uKL` shows a successful dry run
  interrupted by a generic network prompt and repeated `await_job` results
  rendered as errors while their payload said `status: running` or the poll
  timed out. `src/agent/tools.rs::classify_job_result` marks Running and
  Cancelling as unsuccessful command results.
- REL-183 grants only a current-turn, exact-host, closed read-only SSH exception.
  It cannot cover the explicitly requested local dry-run wrapper and adds a
  network-only approval concept that Hermes and MiMo do not use for their
  interactive shell approval flows.
- Read-only oracle evidence: Hermes runs one risk/dangerous-command gate and
  presents once/session/always/deny choices; MiMo evaluates one permission rule
  set for the shell patterns and lets that one ask authorize execution. Neither
  adds a second generic network approval for an already classified command.
- Before the follow-up installation request, the installed
  `/Users/aji/.local/bin/ca` differed from the repository release binary.

## Scope

Allowed files:

- `src/app.rs`
- `src/main.rs`
- `src/permission.rs`
- `src/agent/tools.rs`
- `src/tui/input.rs`
- `src/tui/input_tests.rs`
- `src/tui/render.rs`
- `src/tui/render_tests.rs`
- affected `src/tui/snapshots/*.snap`
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`
- this ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, runtime config/session files, installed binaries,
  secrets, and unrelated dirty-worktree changes.

Runtime state affected: test fixtures and build artifacts only.

## Design and Invariants

- In the interactive TUI, the command's requested network capability is part of
  the same tool decision: Allow/AutoReview/RunEverything executes it, Ask shows
  one command prompt, and Deny blocks it. There is no preliminary network-only
  modal. Headless `--allow-network`, service-client authority, child grants,
  email, and privilege boundaries remain separate and unchanged.
- Slash commands exist only when the composer begins with `/`. A later absolute
  path can open path completion, but never command completion.
- Approval rendering extracts `run_command.command`, sanitizes terminal control
  characters, and emits every character into wrap-capable transcript lines.
- `await_job` treats Running/Cancelling as successful observations of a
  nonterminal job; terminal non-zero, failed, cancelled, or timed-out jobs stay
  unsuccessful.

## Milestones

### M1 — Characterize all four regressions

- Add exact prose-path Enter coverage, long-command approval coverage, live
  network-authority boundaries, and nonterminal await-job classification.
- Validation: exact named tests fail for the old behavior.

### M2 — Implement bounded repairs

- Restrict slash palette activation to a leading command token.
- Render full commands in approval prompts.
- Separate await polling classification from foreground completion.
- Include requested network access in the interactive command decision.
- Validation: exact named tests and related focused suites pass.

### M3 — Reconcile and complete

- Run minimum Rust completion gates and record actual results.
- Update architecture/continuity/testing/queue, move this plan to completed.

## Acceptance Criteria

- Typing the exact reported copy sentence and pressing Enter submits it as
  ordinary user input; no slash palette or `Unknown command` result appears.
- Wide and narrow approval renders contain the final sentinel of a command
  longer than 80 characters and preserve the exact command content.
- Network-capable commands run without a separate prompt in
  AutoReview/RunEverything; Allowlist or an explicit Ask rule shows one command
  approval, and explicit Deny still blocks before execution.
- A one-second `await_job` observation of a longer-running job is a successful
  tool result containing `status: running`; terminal failures remain failures.
- Focused tests, full serial workspace tests, formatting, strict all-target
  all-feature Clippy, and all-feature release build pass or are recorded with
  exact blockers.

## Validation

Focused:

- `cargo test approval --no-fail-fast -- --test-threads=1`: PASS, 84 tests
  across the workspace.
- `cargo test tui::input_tests --no-fail-fast -- --test-threads=1`: PASS, 106
  tests.
- Exact combined-network, SSH/deferred-order, complete-command, and job-status
  regressions: PASS.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS, 2,022 CA
  tests, 38 CLI tests, 17 role-run tests, 556 embedded scheduler tests, all
  integration/doc targets, and three expected manual/external ignores.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.

Manual/visual: deterministic wide/narrow approval and workflow snapshots pass;
no live network call.

Installed-binary acceptance: PASS after separate explicit user authorization;
hash equality, strict macOS signature verification, normal command lookup,
`--help`, and `--version` pass.

## Progress

- [x] Correlated the screenshots with the resumed session and source paths.
- [x] Add failing characterization tests.
- [x] Implement the four bounded repairs.
- [x] Run focused and completion validation.
- [x] Reconcile harness documents and move the plan to completed.

## Decisions

- Decision: Treat the failures as one resumed-task reliability repair.
  Evidence: all four occurred in the same user workflow and span policy,
  composer routing, approval presentation, and asynchronous job reporting.
  Consequence: one ExecPlan owns the cross-module invariants and validation.
- Decision: Follow Hermes/MiMo's single shell-approval decision instead of
  inventing process-local natural-language network authority.
  Evidence: the user explicitly requested the existing Hermes/MiMo behavior;
  both oracles classify the operation once and reuse the resulting decision.
  Consequence: no heuristic inspection of user prose and no new persisted or
  process-local authority state.

## Surprises and Discoveries

- A bounded await timeout retrieves the still-running job correctly, but the
  shared terminal classifier immediately converts that observation into a
  failed tool result.
- Approval argument clipping is intentional renderer code, not a terminal-width
  limitation.

## Recovery and Rollback

Revert only this plan's local hunks with an inverse patch. Do not reset the
dirty worktree. Snapshot changes are confined to approval fixtures.

## Completion Record

Changed files: `AGENTS.md`, `src/approval.rs`, `src/main.rs`, `src/permission.rs`,
`src/agent/tools.rs`, `src/tui/input.rs`, `src/tui/input_tests.rs`,
`src/tui/render.rs`, `src/tui/render_tests.rs`, six affected approval/workflow
snapshots, and the listed harness documents.

Commands and results: focused and completion gates above all passed. The only
diagnostic was the existing macOS debug-linker compact-unwind size warning;
strict Clippy and release linking passed.

Installed delivery: after explicit follow-up authorization, the validated
release was installed at `/Users/aji/.local/bin/ca` with SHA-256
`b1e029ff2a87d691f1eabd55a6a5960480ce0969096508d5571a58f0c7545591`.
The prior binary is recoverable at
`/Users/aji/.local/bin/ca.backup-rel185-20260826` with SHA-256
`3aa2d455caf2ec1e22302a154964dd80a4d750469b94e29e1c579162d852c56a`.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`,
`TESTING.md`, and `docs/.INDEX.md` record the completed behavior and evidence.
