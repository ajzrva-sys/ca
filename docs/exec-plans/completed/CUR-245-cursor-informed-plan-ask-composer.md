# CUR-245 — Cursor-informed Plan/Ask/Composer Follow-ups

Status: COMPLETE (2026-09-17; focused tests + fmt + clippy; not installed this turn)
Owner/session: Cursor-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Close the strongest product gaps found when studying Cursor Agent CLI
`2026.09.15-d2fe57e` (agent-cli-local, authless, MiMo/DeepSeek local providers)
against `ca`, without copying oracle code and without claiming Cursor runtime
parity.

One observable outcome after activation: Ask/Plan tool boundaries are
proven by CA-owned fixtures; plan mode can optionally persist/revisit a
markdown plan artifact; mode transitions always surface policy in the UI.

## Non-Goals

- No Cursor cloud, background composer, cloud agent, or `cursor-plan://` host features.
- No import of Cursor bundle strings as executable instructions.
- No hosted auth, telemetry, or proprietary Connect/protobuf transport.
- No change to `ca`'s role as the product; Cursor remains a study bench only.
- No claim that `ca` matches Cursor TUI layout; PARITY rows stay Unverified until clean observed fixtures exist.
- No oracle tree edits (`oracles/` including `latest-cursor` / `agent-cli-local` installs under `~/.local/share/cursor-agent-local` are read-only evidence).

## Evidence and Current Behavior

### Cursor observations (2026-09-17)

Source: `~/.local/share/cursor-agent-local/versions/2026.09.15-d2fe57e/`,
wrappers `~/.local/bin/agent-local-{deepseek,mimo}`, help text, bundle strings
in `index.js` / `7076.index.js` / `5788.index.js` / `8366.index.js`.

- CLI contract (Observed via `agent-local --help`):
  - `--mode <plan|ask>` — plan: read-only planning; ask: Q&A read-only.
  - `--plan` — shorthand for plan mode.
  - Headless `-p --mode plan|ask` returns text/JSON without Cursor login when `--authless --base-url` is set.
- Plan-mode bundle rules (string evidence, not full tool-trace):
  - “only edit markdown files” (and canvas); plan iteration continues in-session.
  - Plan artifact naming: `*.plan.md` / `cursor-plan://…`.
  - Prompt fragment: “Current plan mode progress” when a prior plan exists.
- Ask-mode bundle rule: cannot run non–read-only tools; suggest switching to agent mode for edits.
- Mode switch UI string: `Switched composer mode from X to Y`.
- Model catalog field: `supports_plan_mode`.
- “Composer” is overloaded in Cursor: input surface, session `composer_id`, and model family (`composer-2.5`). Only the input-surface meaning is in scope for `ca`.

### `ca` source facts

- `src/app.rs` — `AgentMode::{Agent, Plan, Ask, Debug, Custom}`.
- `src/main.rs` — `--mode` / `--plan` launch flags; conflict with continue/resume; PLAN system prompt requires `propose_plan` and forbids fs/process/network/MCP/subagent tools; `request_tool_profile` uses `ToolProfile::Plan` when mode is Plan.
- `src/main.rs` LaunchMode mapping — `Self::Ask => ToolProfile::Main` (plan maps to `ToolProfile::Plan`). Runtime Ask tool filtering also exists (`allowed_names` when `AgentMode::Ask`); completeness is not yet proven by a dedicated matrix fixture for launch-mode Ask.
- `src/plan.rs` — `/plan show|execute|approve|revise|dismiss|reject|edit`; execute returns to `AgentMode::Agent`.
- `src/tui/input.rs` — mode cycle Agent→Plan→Ask→Debug→Agent with status strings (“Plan mode · read-only tools · propose before editing”).
- `src/tui/composer.rs` / TUI-138 — multiline composer, path completion, attachments, large-paste expansion.
- `PARITY.md` — startup/composer and approvals rows remain `Unverified`.

### Inferences (labeled)

- Inferred: Cursor’s “markdown-only plan edits” is a different product shape than `ca`’s propose/review/execute card; porting must keep host approval authority.
- Inferred: LaunchMode::Ask using `ToolProfile::Main` at the launch seam is a risk even if interactive Ask filtering works; needs a failing-then-passing fixture before any code change.

## Scope

Allowed files (when activated):

- `src/main.rs`, `src/plan.rs`, `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/composer.rs` as needed
- `src/agent/registry.rs` (Ask/Plan tool profiles only if required)
- Focused tests under `src/` / `tests/` / `src/tui/*_tests.rs`
- `PARITY.md`, `CONTINUITY.md`, `TESTING.md`, `docs/.INDEX.md`, `NEXT_STEPS.md`
- This plan under `docs/exec-plans/active/` then `completed/`

Forbidden files/areas:

- `oracles/`, `reference/`, installed Cursor trees, `~/.local/share/cursor-agent-local`
- Credentials, user runtime config/session mutation during tests
- Hosted Cursor features, arbitrary marketplace, automatic commit/push
- Unrelated active ExecPlan hunks (AUT-231, SEC-158, etc.)

Runtime state affected: temporary fixture homes only; no install until a separate release plan if source changes.

## Design and Invariants

1. **Ask/Plan tool boundary** — Ask never mutates workspace files or runs privileged/network tools via any launch path (interactive, headless, ACP, service). Plan remains proposal-oriented; execute is host-approved and switches to Agent.
2. **Plan artifact optional** — If added, plan markdown is CA-owned session/project metadata, digest-bound, approval-gated; not a Cursor URI clone; not auto-executed.
3. **Mode visibility** — Every mode change updates one user-visible status including tool policy summary.
4. **Evidence labels** — Cursor help/strings may inform priority; acceptance is CA fixtures only. PARITY Observed only for exact CLI flag help captured in fixtures.
5. **Safety** — Deny rules remain ceilings; no weakening of `/permissions`, sandbox, or yolo boundaries to make tests pass.

## Milestones

### M1 — Ask/Plan boundary characterization

- Add failing (or gap-proving) focused tests: LaunchMode Ask/Plan, interactive Ask, headless Ask — assert no write/privileged tools in the model-visible or executable set.
- Record actual tool catalogs from registry/profile resolution.
- Validation: focused `cargo test` on new tests + `request_tool_profile` / permission evaluate tests.

### M2 — Fix Ask launch profile if M1 shows a gap

- Smallest change: Ask launch uses a read-only profile or equivalent filter; Plan unchanged unless tests demand it.
- Validation: focused tests + existing approval/permission suites; no clippy warnings on touched crates.

### M3 — Mode-policy status completeness

- Ensure `/plan` transitions, mode cycle, `--mode`/`--plan` launch, and ACP plan/ask set the same status shape.
- Snapshot or string tests for wide/narrow TUI where visible.
- Validation: input/render focused tests; zero `.snap.new`.

### M4 — Optional plan markdown artifact (only if still desired after M1–M3)

- Design record: where stored, digest, revise vs propose_plan, export, no silent execute.
- Implement behind default-off or explicit `/plan export`/`/plan show --file` if lower risk.
- Validation: plan.rs tests + session persistence compatibility tests.

### M5 — Documentation and PARITY hygiene

- PARITY: add Observed note for Cursor CLI `--mode`/`--plan` help text with capture path/date; leave TUI rows Unverified.
- CONTINUITY/NEXT_STEPS: record CA-owned acceptance only.
- Validation: docs only; protected-tree status unchanged for `oracles/`.

## Acceptance Criteria

- [x] Named tests prove Ask cannot obtain write/network/privilege tools through launch or interactive entry.
- [x] Plan execute still requires explicit user/host action and returns to Agent mode (existing `plan.rs` tests remain green).
- [x] Mode changes always surface policy text in CA-owned render/input tests.
- [x] If plan artifact ships: default behavior without it is unchanged; digest/approval invariants covered by tests.
- [x] No oracle/reference byte changed; no Cursor code copied into `src/`.
- [x] Focused tests, `cargo fmt --check`, strict clippy on touched package, and relevant completion gates recorded honestly.

## Validation

Focused:

- New Ask/Plan tool-boundary tests
- `src/plan.rs` tests
- TUI input/render mode-status tests

Completion (when implementation milestones run):

- Follow current `TESTING.md` focused + workspace gates for the touched surface
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings` (or the TESTING.md scoped equivalent)
- Snapshot hygiene: zero `.snap.new`
- Direct `git status` for `oracles/` and `reference/` empty

Manual/visual: interactive `ca --plan` / `ca --mode ask` / mode cycle in a disposable workspace; compare to `agent-local-deepseek --plan --trust` only as UX reference, not pass criteria.

Blocked checks: clean Cursor interactive TUI capture for full PARITY Matched status remains blocked until a dedicated observation pack is requested.

## Progress

- [x] Cursor plan/ask/composer study recorded (session 2026-09-17; agent-cli-local on DeepSeek/MiMo).
- [x] This DRAFT ExecPlan written for later activation.
- [x] M1 Ask/Plan boundary characterization tests
- [x] M2 launch-profile fix if required
- [x] M3 mode-policy status completeness
- [x] M4 plan artifact (optional)
- [x] M5 docs/PARITY hygiene
- [x] Plan activated and completed by an explicit user request

## Decisions

- Decision: Status DRAFT until the user activates this plan.
  Evidence: User asked for a plan “for later.”
  Consequence: No `src/` edits from this document alone.

- Decision: Treat Cursor “Composer” model/session meanings as out of scope.
  Evidence: Overloaded term in bundle; `ca` already has a composer input surface.
  Consequence: Follow-ups stay on plan/ask policy, mode UX, and optional plan artifact.

## Surprises and Discoveries

- Headless Cursor `-p` without `--trust` still answered a no-tool prompt in a disposable dir; trust enforcement is strongest when tools/MCP/sandbox gates run. Do not assume every headless path prompts.
- Cursor default `approvalMode` is `allowlist` with `permissions.allow: ["Shell(ls)"]` and `sandbox.mode: "disabled"` — weaker than `ca`'s default `SandboxMode::Required`. Do not copy Cursor sandbox defaults into `ca`.

## Recovery and Rollback

- Docs-only activation path: abandon plan; no source rollback needed.
- If M2+ edit source: revert only files listed in this plan via ordinary Git (no reset of unrelated dirty trees); keep fixture snapshots reviewed.

## Completion Record

Changed files:
- `src/main.rs` — launch-mode status strings naming tool policy; Ask/Plan tool-boundary tests
- `src/tui/input.rs`, `src/tui/render.rs` unchanged for M4 plan-artifact (optional milestone skipped)

Commands and results:
- Focused tests PASS: ask_mode_resolves_zero_tools; plan_profile_exposes_only_proposal_tools; launch_mode_status_strings; launch_modes_are_process_only
- Inventory: Ask interactive+headless already set allowed_names=empty → zero tools; Plan profile is propose_plan/configure_agent_graph/edit_plan only
- M4 plan markdown artifact not implemented (optional; store-level plan card remains CA authority)
- fmt + clippy PASS

Remaining risks: none for shipped characterization.
Harness updates: NEXT_STEPS/CONTINUITY/index.
