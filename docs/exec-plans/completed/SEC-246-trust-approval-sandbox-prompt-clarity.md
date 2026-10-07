# SEC-246 — Trust/Approval/Sandbox Prompt Clarity

Status: COMPLETE (implemented 2026-09-17; docs+focused tests)
Owner/session: Cursor-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Improve `ca`'s trust, approval-autorun, and sandbox **user-facing clarity** using
Cursor Agent CLI `2026.09.15-d2fe57e` as a product-study bench only — without
adopting Cursor's weaker defaults (sandbox off, remote Smart Auto, casual MCP
auto-approve) and without claiming Cursor runtime parity.

One observable outcome after activation: CA-owned tests prove (1) untrusted /
blocked-tool messages name the path and the exact recovery options, (2) autorun
flags/modes cannot silently combine without a defined winner or a fail-closed
error, (3) auto-review and sandbox availability are labeled truthfully in
doctor/status surfaces.

## Non-Goals

- No Cursor cloud/team "Run Everything" policy, no remote classifier requirement.
- No defaulting `ca` sandbox to off or copying Cursor `cli-config.json` defaults.
- No `--approve-mcps`-style blanket MCP auto-approve as a casual default.
- No oracle/reference edits; no Cursor bundle strings copied as executable instructions or as required UI copy (paraphrase is allowed; fixtures define acceptance).
- No PARITY `Matched` claims for approvals/TUI without a separate clean observation pack.
- No unrelated permission-engine redesign beyond label/exclusivity/prompt seams.

## Evidence and Current Behavior

### Cursor observations (2026-09-17)

Sources: `agent-local --help`; bundle chunks
`~/.local/share/cursor-agent-local/versions/2026.09.15-d2fe57e/{index.js,7076.index.js,5788.index.js,8366.index.js}`;
runtime headless probes with disposable HOME; generated
`~/.cursor/cli-config.json` under that HOME.

- Workspace Trust Required prompt names the directory and recovery:
  interactive decide, or `--trust` / `--yolo` / `-f`.
- Trust marker observed: `~/.cursor/projects/<slug>/.workspace-trusted`.
- Headless `-p` with a no-tool prompt still succeeded **without** `--trust` in a
  disposable dir — trust enforcement is strongest on tool/MCP/sandbox paths.
- Approval modes: Allowlist (default), Auto-review (Smart Auto, server
  classifier), Run Everything (`-f`/`--yolo` / `approvalMode: unrestricted`).
- Force/yolo and auto-review are **mutually exclusive** (explicit CLI error).
- Default config: `approvalMode: "allowlist"`, `permissions.allow: ["Shell(ls)"]`,
  `sandbox.mode: "disabled"`.
- `--sandbox enabled|disabled`; `cursorsandbox --preflight-only` exists.
- `--approve-mcps` auto-approves MCP servers.
- Bundle: team admin may disable Run Everything (hosted; out of scope to copy).

### `ca` source facts

- `src/config.rs` — `trusted_workspaces: Vec<String>`; `trust_workspace()` persists paths idempotently.
- `src/main.rs` — `--trust` is process-only fresh-workspace authority; cannot combine with continue/resume.
- `src/permission.rs` — `PermissionDecision::{Allow,Ask,Deny}`; deny ceilings; `auto_review_safe_invocation` / `auto_review_safe_command` (local safe-tool/command classifier, not a remote service).
- `src/sandbox.rs` — `SandboxMode::{Required, PolicyOnly, Off}`; unknown parse defaults to `Required`; `Required` uses `/usr/bin/sandbox-exec`.
- CLI (`ca --help`) — `--yolo` bypasses interactive approval prompts (boundaries still apply); `--sandbox required|policy-only|off`; `--trust` process-only.
- User runtime config already sets `agent.approval_mode = "auto-review"` and `agent.sandbox_mode = "required"` in `~/.config/ca/config.toml`.
- `PARITY.md` — approvals row remains `Unverified`; `/sandbox` and `/permissions` are CA extensions informed by oracles, not Cursor parity claims.

### Inferences (labeled)

- Inferred: Cursor's trust copy is a clarity benchmark, not a security benchmark; `ca` defaults are already stricter.
- Inferred: Combining CA `--yolo` with config `approval_mode = "auto-review"` may have a defined winner in code; if so it needs a named test. If not, it is a real gap.
- Inferred: "Auto-review" is easy to misread as Cursor Smart Auto; CA should label it as local safe-tool auto-review.

## Scope

Allowed files (when activated):

- `src/main.rs` (CLI conflict checks, launch/status messages)
- `src/permission.rs` / `src/approval.rs` only as needed for exclusivity tests or status labels
- `src/sandbox.rs` / doctor-related modules for preflight/availability labels
- `src/tui/render.rs` / status-message seams for trust/sandbox labels if visible
- Focused tests and snapshots
- `PARITY.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, this plan

Forbidden files/areas:

- `oracles/`, `reference/`, Cursor install trees
- Credentials, real user config mutation in tests (use temp homes)
- Weakening deny ceilings, sandbox Required default, or MCP fail-closed behavior
- Unrelated active ExecPlan hunks (AUT-231, SEC-158, CUR-245, etc.)

Runtime state affected: disposable test homes only; no binary install unless a later release plan is authorized.

## Design and Invariants

1. **Stricter defaults stay** — `SandboxMode::Required` and deny ceilings are not weakened to match Cursor.
2. **Truthful labels** — Any auto-review surface must say it is CA-local safe-tool/command evaluation, not a remote classifier.
3. **Fail closed on ambiguous autorun** — If two autorun mechanisms conflict and precedence is not already specified, error clearly instead of silently widening authority.
4. **Trust messages are CA-owned** — Name workspace path, what is blocked, and recovery (`--trust` process-only vs adding `trusted_workspaces` vs interactive confirm). Acceptance is CA snapshot/string tests.
5. **MCP trust stays separate** from workspace trust unless a prompt merely cross-references both; no merged authority.
6. **Evidence discipline** — Cursor observations inform priority only; no oracle byte changes; no parity promotion without fixtures in `PARITY.md` order.

## Milestones

### M1 — Characterization (failure-first where behavior is unclear)

- Tests: CLI/launch combination of `--yolo` with configured auto-review — record actual winner or error.
- Tests: untrusted-workspace / blocked-tool status or headless refusal copy includes path + recovery (whatever CA currently emits; may start as documenting current text).
- Tests: `SandboxMode` doctor/status label when backend missing vs present (may already exist — extend only if gaps).
- Validation: focused `cargo test` on new/updated tests.

### M2 — Autorun exclusivity / precedence

- If M1 shows silent combination: implement fail-closed error or documented single winner with status text.
- If already exclusive: lock with a test and a one-line help/docs clarification.
- Validation: focused CLI/permission tests; fmt + clippy on touched code.

### M3 — Prompt/status clarity

- CA-owned trust/refusal wording (path + blocked surface + recovery options).
- Auto-review labeling in status/doctor/permissions help text as local safe-tool auto-review.
- Sandbox availability: clear pass/fail-closed language when `sandbox-exec` cannot be used under `Required`.
- Validation: string/snapshot tests; no `.snap.new` pending.

### M4 — PARITY/harness hygiene

- PARITY: optional Observed note for Cursor **CLI help flags** only (`--trust`, `--yolo`, `--auto-review`, `--sandbox`) with capture date/version; approvals TUI row stays Unverified.
- CONTINUITY/NEXT_STEPS updates; move plan to completed/ when done.
- Validation: docs; direct `oracles/` status empty.

## Acceptance Criteria

- [ ] Named test states the defined interaction of `--yolo` vs auto-review (error or documented winner); no silent widen.
- [ ] Trust/blocked-tool messages tested to include workspace path and recovery options.
- [ ] Auto-review is labeled as local in at least one user-visible help/status surface.
- [ ] Sandbox Required failure path remains fail-closed with clear language (fixture).
- [ ] Defaults still: no sandbox-off by default; no MCP auto-approve default.
- [ ] No oracle/reference changes; no Cursor source copied into `src/`.
- [ ] Focused tests + fmt + scoped clippy + snapshot hygiene recorded honestly.

## Validation

Focused:

- CLI conflict/launch tests
- Permission/auto-review label tests
- Sandbox doctor/status tests
- TUI/status string tests if UI text changes

Completion (when implementation milestones run):

- Follow `TESTING.md` for touched surface
- `cargo fmt --all -- --check`
- Strict clippy as documented in TESTING.md
- Zero `.snap.new`
- `git status` clean for `oracles/` and `reference/`

Manual/visual: headless refusal in an untrusted disposable workspace; `ca --doctor` sandbox/auto-review lines; optional compare to `agent-local-deepseek` trust prompt as UX reference only.

Blocked checks: Cursor remote Smart Auto classifier behavior; team-admin Run Everything policy; full Cursor TUI approval parity.

## Progress

- [x] Approval/trust/sandbox audit recorded (session 2026-09-17).
- [x] This DRAFT ExecPlan written for later activation.
- [x] M1 characterization tests
- [x] M2 autorun exclusivity/precedence (clap conflicts_with locked by tests)
- [x] M3 prompt/status clarity (trust prompt, /auto-review labels, doctor approval/sandbox copy)
- [x] M4 PARITY Observed CLI notes in PARITY.md
- [x] Activated and completed 2026-09-17

## Decisions

- Decision: Status DRAFT until the user activates this plan.
  Evidence: User asked for SEC-246 drafted the same way as CUR-245.
  Consequence: No `src/` edits from this document alone.

- Decision: Do not adopt Cursor sandbox/approval defaults.
  Evidence: Cursor generated config has `sandbox.mode: "disabled"` and tiny allowlist; `ca` uses `Required` + richer permissions.
  Consequence: Scope is clarity and fail-closed combination behavior only.

## Surprises and Discoveries

- Cursor headless no-tool prompts may not require workspace trust; do not assume every refusal path is equivalent.
- Cursor explicitly forbids combining Run Everything and Auto-review — useful CA test shape even if CA modes differ.

## Recovery and Rollback

- DRAFT abandon: no source rollback.
- Implementation: revert only files listed in Scope via normal Git; do not reset unrelated dirty trees.

## Completion Record

Changed files:
- `src/main.rs` — help text, trust status path, `/auto-review` local labels, CLI tests
- `src/tui/render.rs` — workspace trust prompt (path, block reason, y/n recovery, headless `--trust` note)
- `src/config.rs` — `ProviderManagementSnapshot.approval_mode`
- `src/doctor.rs` — `approval_check`, clearer sandbox fail-closed copy
- `src/tui/render_tests.rs` — trust prompt assertions
- `PARITY.md` — SEC-246 Observed CLI notes

Commands and results:
- `cargo test --bin ca` focused: yolo/auto-review conflict, flag-alone parse, trust+continue, local auto-review label, provider snapshot, doctor offline report, trust prompt render — PASS
- `cargo fmt --all` — applied
- `cargo clippy --bin ca --all-targets -- -D warnings` — PASS
- Snapshot hygiene: no trust-prompt `.snap` (temp paths unstable; assertions only); zero `.snap.new`

Remaining risks: none for this scope; child Cursor plans still DRAFT.
Harness updates: NEXT_STEPS SEC-246 COMPLETE; plan moved to completed/.

