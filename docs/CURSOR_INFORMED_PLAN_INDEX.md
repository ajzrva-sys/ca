# Cursor-Informed Plan Index

Last updated: 2026-09-17  
Index plan: `docs/exec-plans/completed/IDX-250-cursor-informed-draft-plan-index.md`  
Index status: **COMPLETE** — Cursor children all COMPLETE; **HRS-250** (Hermes Tier-1 context/cache) also COMPLETE 2026-09-17.  
Study bench: Cursor Agent CLI **`agent-cli-local` `2026.09.15-d2fe57e`**  
Wrappers: `agent-local-deepseek`, `agent-local-mimo` (CA Keychain providers; **no Cursor account**)

`ca` remains the product. Cursor is a **behavior study bench** for later CA-owned work.

## Cloud vs local (all children)

| Works on local provider (study OK) | Needs Cursor cloud/account (non-goal unless product expands) |
|---|---|
| Chat/tools, plan/ask, trust/sandbox prompts, slash/TUI patterns, local session resume, `models` subcommand, MCP **list**, plugin marketplace **list** | `login`, `--list-models`, Cloud Agent `cloud`/`worker`, team Run Everything, hosted background composer, Cursor OAuth/telemetry/update channels |

Acceptance for every child plan is **CA-owned tests/fixtures only**. No oracle source copy; no Cursor parity `Matched` without `PARITY.md` evidence order.

## DRAFT plans (not activated)

| ID | Title | Path | Study source | Goal (one line) | Non-goals (short) |
|---|---|---|---|---|---|
| **CUR-245** | Cursor-informed plan/ask/composer follow-ups | `docs/exec-plans/completed/CUR-245-cursor-informed-plan-ask-composer.md` | Startup, `--mode plan\|ask`, `--plan`, composer | **COMPLETE** — Ask zero-tool + Plan proposal-only fixtures; launch mode status | No cloud composer; no Cursor markdown-only loopholes in Agent mode |
| **SEC-246** | Trust/approval/sandbox prompt clarity | `docs/exec-plans/completed/SEC-246-trust-approval-sandbox-prompt-clarity.md` | Workspace Trust, allowlist / auto-review / yolo, `--sandbox` | **COMPLETE** — trust prompt clarity; yolo/auto-review exclusivity tests; local auto-review labels in help/status/doctor | Do not adopt Cursor defaults (`sandbox` off, remote Smart Auto) |
| **TUI-247** | Tool lifecycle events and tool-card clarity | `docs/exec-plans/completed/TUI-247-tool-lifecycle-events-and-card-clarity.md` | Headless `stream-json` `tool_call started/completed`; CA tool/diff/interrupt snapshots | **COMPLETE** — ToolStart/ToolFinish already present; characterization test | No Cursor classifier authority; TUI-240 owns interrupt/queue product |
| **CMD-248** | Slash/picker/keymap hygiene | `docs/exec-plans/completed/CMD-248-slash-picker-keymap-hygiene.md` | Cursor CLI commands + slash strings; CA palette/keymap snaps | **COMPLETE** — descriptions + model-as-slash alias + /tasks usage | Do not shrink CA registry; no `/run-everything` productization |
| **SES-249** | Headless session identity and resume fixtures | `docs/exec-plans/completed/SES-249-headless-session-identity-and-resume.md` | Cursor `--resume`/`--continue`/`persist`; headless `PERSIST_PROBE_1` recall | **COMPLETE** — store resume-memory fixtures; CLI new-session-id/no-session exclusivity | No Cursor SQLite chat store; no persist-as-SessionV4 |

## Suggested activation order

1. **SEC-246 M1** or **CUR-245 M1** — characterization first (safety/mode boundaries).  
2. **SES-249 M2** — headless resume-memory (high confidence, small surface).  
3. **CMD-248 M1/M3** — hygiene/docs.  
4. **TUI-247 M1** — inventory existing serve/headless events before adding any.  
5. Optional deeper milestones only after M1 inventories.

Activate **one** plan at a time (harness rule). Do not run children in parallel unless the user explicitly reassigns ownership.

## Evidence paths (session)

| Topic | Where |
|---|---|
| Observation HOME / logs / mock | `/tmp/ca-cursor-offline-obs/` |
| Cursor local runtime | `~/.local/share/cursor-agent-local/versions/2026.09.15-d2fe57e/` |
| Cloud CLI extract | `oracles/latest-cursor/` + `~/.local/share/cursor-agent/…` (agent-cli, not local) |
| Wrappers | `~/.local/bin/agent-local-provider`, `agent-local-deepseek`, `agent-local-mimo` |
| Provider resolve | `~/.local/share/ca-agent-local/resolve_provider.py` |
| CA session/plan/TUI source | `src/plan.rs`, `src/session.rs`, `src/tui/*`, `src/permission.rs`, `src/sandbox.rs` |
| PARITY rows still Unverified | startup/composer, approvals, slash/pickers, diff/tool cards, tool result loop |

## Study conclusions (summary)

| Area | Verdict vs `ca` |
|---|---|
| Plan/ask/composer | CA has modes + composer; gaps are Ask launch profile proof + optional plan artifact |
| Trust/approval/sandbox | CA defaults **stricter**; port clarity, not Cursor defaults |
| Tool cards / diffs / interrupt | CA TUI **ahead**; Cursor denser **headless events** |
| Slash / pickers / keymap | CA registry/keymap **ahead**; optional model-as-slash |
| Session resume/persist | Cursor headless resume works; CA session **product** already deeper |
| Cloud vs local | Local provider covers study surface; cloud/worker/login are non-goals |

## How to use this index

1. Pick one DRAFT plan above.  
2. Read that plan’s Evidence / Scope / Acceptance.  
3. Set Status to ACTIVE in the plan file; add/keep `NEXT_STEPS.md` link.  
4. Follow `.agent/PLANS.md`: failing characterization first, CA fixtures, no oracle edits.  
5. On completion move the plan to `docs/exec-plans/completed/` and update this index’s status table.
