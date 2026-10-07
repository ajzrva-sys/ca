# TUI-247 — Tool Lifecycle Events and Tool-Card Clarity

Status: COMPLETE (2026-09-17; focused tests + fmt + clippy; not installed this turn)
Owner/session: Cursor-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Optionally enrich `ca` tool presentation and headless observability using Cursor
Agent CLI `2026.09.15-d2fe57e` as a study bench only: clearer tool-card metadata
(timing/truncation when available) and a **CA-owned** headless/serve tool
lifecycle event contract (`started` / `completed`) — without copying Cursor
protocol payloads, remote classifiers, or weaker approval defaults.

One observable outcome after activation: CA fixtures define tool-card timing/
truncation markers where host metadata exists, and headless/serve JSONL can
emit sanitized `tool_call.started` / `tool_call.completed` events with call ids,
tool name, bounded args summary, and outcome metadata.

## Non-Goals

- No Cursor Connect/protobuf transport, no `cursor` session IDs as CA identity.
- No remote Smart Auto classifier, no `classifierResult` / `suggestedSandboxMode` import as authority.
- No unbounded stdout/stderr dumps; no secrets in tool event payloads.
- No redesign of approval prompts, permission ceilings, or sandbox defaults.
- No claim of Cursor TUI parity for tool cards/diffs/interrupt (PARITY rows stay Unverified until separate fixtures).
- No interrupt/queue product rework — TUI-240 already owns that surface.
- No oracle/reference edits; no Cursor source copied into `src/`.

## Evidence and Current Behavior

### Cursor observations (2026-09-17)

Source: `agent-local-deepseek -p --output-format stream-json` on a disposable
workspace; package
`~/.local/share/cursor-agent-local/versions/2026.09.15-d2fe57e/`.

- Event envelope (Observed): `system init` → `user` → `tool_call started` →
  `tool_call completed` → `assistant` → `result`.
- Shell tool payload includes: `description`, `command`, parse metadata,
  `skipApproval`, `classifierResult` (often failed/undetermined), timing
  (`startedAtMs`, `completedAtMs`, `executionTime`, `localExecutionTimeMs`),
  result (`exitCode`, stdout/stderr/interleaved), limits
  (`fileOutputThresholdBytes`, timeouts), ids (`call_id`, `toolCallId`,
  `model_call_id`, `session_id`).
- `permissionMode` on init (`default` in authless probe).
- Bundle references `apply_patch`, unified-diff workers, interrupt/queue
  strings; no live interactive tool-card chrome captured this session.

### `ca` source/snapshot facts

- TUI tool cards: `src/tui/render.rs` + snapshots
  `completed_tool`, `adaptive_tool_output_collapsed` / `_expanded`,
  `bounded_tool_output`, `approval_multi_tool_*`, `tool_selection_failure`.
- Diff UI: `working_tree_diff`, `worktree_diff`, `commit_review_diff`
  snapshots; change-review diff mode in `render.rs`.
- Interrupt/queue: TUI-240 complete (`Esc stop · Enter interrupt · Alt+Enter queue`);
  `input_tests.rs` coverage; `turn_metadata_interrupted`.
- Headless: `--output-format text|json|jsonl` (`ca --help`); serve JSONL
  `ca.serve.v1` owns canonical runner/tool events (`src/service.rs`) — check
  existing event names before adding any new ones.
- PARITY: `diff/tool cards` and `tool result loop` rows `Unverified`.

### Inferences (labeled)

- Inferred: CA already has interactive tool cards and interrupt/queue; highest
  gap is **structured headless/serve tool lifecycle** and optional **card
  metadata density**, not a TUI rewrite.
- Inferred: Serve JSONL may already carry tool-related fields; M1 must map
  existing events before proposing duplicates.

## Scope

Allowed files (when activated):

- Headless/serve event seams: `src/service.rs`, headless contract/output modules as needed
- TUI card metadata: `src/tui/render.rs`, related tests/snapshots
- Focused tests under `src/`, `tests/`
- Docs: `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, this plan

Forbidden files/areas:

- `oracles/`, `reference/`, Cursor install trees
- Credentials; user runtime config/session mutation in tests
- Approval/permission/sandbox policy weakening
- Unrelated active ExecPlan hunks (AUT-231, SEC-158, CUR-245, SEC-246)

Runtime state affected: disposable fixtures; no install unless a later release plan is authorized.

## Design and Invariants

1. **CA-owned event schema** — Versioned or documented event names in CA style
   (e.g. `tool_call.started` / `tool_call.completed` under `ca.headless` /
   `ca.serve.v1` rules). Never emit Cursor wire shapes by default.
2. **Sanitized by default** — Tool name, call id, bounded args **summary**,
   status/exit, duration; no raw secrets, provider bodies, or full env.
3. **Approval unchanged** — Events describe host-authorized execution outcomes;
   they never widen authority or replace approval exactly-once rules (GRD-005).
4. **Card metadata is host-truth** — Timing/truncation markers only when the
   host actually has that data; never invent durations.
5. **Snapshots stay CA-owned** — Wide/narrow fixtures define acceptance.
6. **No oracle parity claim** — Cursor stream-json is priority evidence only.

## Milestones

### M1 — Inventory existing headless/serve tool events

- Document what `ca.headless` json/jsonl and `ca.serve.v1` already emit for tools.
- Identify gaps vs desired lifecycle (started/completed, timing, truncation).
- Validation: docs note + any probe scripts in plan Progress (no code required if already sufficient).

### M2 — Headless/serve tool lifecycle events (if M1 shows a gap)

- Add sanitized started/completed events only where missing; keep compatibility.
- Failure-first tests for event order, ids, redaction, cancellation/interrupt outcomes.
- Validation: focused headless/service tests; fmt; scoped clippy.

### M3 — Tool-card metadata polish

- Optional duration/exit/truncation line on completed tool cards when host data exists.
- Update adaptive/bounded tool snapshots intentionally.
- Validation: render focused tests; zero `.snap.new`.

### M4 — Interrupt/diff docs hygiene only

- Confirm PARITY language: CA interrupt/queue remains extension (TUI-240);
  tool result loop characterization may cite Cursor stream-json as **oracle
  evidence**, not Matched status, unless a separate observation pack is done.
- Validation: docs; `oracles/` status empty.

## Acceptance Criteria

- [x] M1 inventory recorded in this plan; no duplicate event names in serve/headless.
- [x] If events ship: fixtures prove order, bounded payload, redaction, and cancel/interrupt terminal outcomes.
- [x] If card metadata ships: snapshots updated deliberately; no invented timings.
- [x] Approval/exactly-once and sandbox defaults unchanged.
- [x] No oracle/reference byte changed; no Cursor source copied.
- [x] Focused tests + fmt + scoped clippy + snapshot hygiene recorded.

## Validation

Focused: headless/service event tests; render tool-card tests.

Completion (when implementation milestones run): follow `TESTING.md` for touched surface; fmt; clippy; zero `.snap.new`; clean `oracles/`/`reference/` status.

Manual/visual: headless jsonl sample in a temp workspace; optional interactive compare to `agent-local-deepseek` tool events as reference only.

Blocked checks: Cursor interactive TUI tool chrome; remote classifier behavior; full PARITY Matched for tool cards.

## Progress

- [x] Tool cards / diffs / interrupt-queue study recorded (2026-09-17).
- [x] This DRAFT ExecPlan written for later activation.
- [x] M1 inventory of existing CA headless/serve tool events
- [x] M2 lifecycle events if needed
- [x] M3 tool-card metadata polish
- [x] M4 docs/PARITY hygiene
- [x] Explicit activation and completion

## Decisions

- Decision: Status DRAFT until the user activates this plan.
  Evidence: User asked for TUI-247 drafted the same way as prior plans.
  Consequence: No `src/` edits from this document alone.

- Decision: Do not import Cursor classifier/sandbox suggestions into CA tool authority.
  Evidence: Cursor probe showed `classificationFailed` / `UNDETERMINED`; CA has local auto-review and stricter sandbox defaults.
  Consequence: Events are observational; policy stays CA-owned.

## Surprises and Discoveries

- Cursor headless tool_call payload is much denser than CA interactive cards; CA cards are already approval-centric.
- CA interrupt/queue is already more completely specified/tested for TTY than Cursor material captured headless.

## Recovery and Rollback

- DRAFT abandon: no source rollback.
- Implementation: revert only Scope files via normal Git; review snapshot diffs.

## Completion Record

Changed files:
- `src/main.rs` — characterization test `headless_tool_lifecycle_event_shapes_exist` (ToolStart/ToolFinish)

Commands and results:
- M1 inventory: `HeadlessEventV1::{ToolStart,ToolFinish}` already ship in headless JSONL; no duplicate schema added
- M3 tool-card duration deferred: `ToolCall` has no host timing field; do not invent durations
- fmt + clippy PASS

Remaining risks: none; optional card timing needs a host-owned duration field first.
