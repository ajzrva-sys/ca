# CMD-248 — Slash/Picker/Keymap Hygiene

Status: COMPLETE (2026-09-17; focused tests + fmt + clippy; not installed this turn)
Owner/session: Cursor-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Harden `ca` slash-command discovery, optional model-as-slash aliases, and
keymap/help clarity using Cursor Agent CLI `2026.09.15-d2fe57e` as a naming/
UX study bench only — without shrinking CA's command registry, without
adopting Cursor's weaker autorun defaults, and without claiming Cursor TUI
parity for pickers or keybindings.

One observable outcome after activation: every visible slash command has
palette/help coverage in fixtures; optional model-id alias path (if shipped)
is strictly subordinate to `/model`; PARITY records Observed notes only for
Cursor CLI help / empty `mcp list` strings that were actually captured.

## Non-Goals

- No Cursor `/run-everything` productization; no weakening of `/permissions`,
  `/sandbox`, or approval defaults.
- No removal or hiding of CA commands to "match" Cursor's smaller TUI set.
- No vim-mode clone beyond CA's existing keymap manager; no Cursor config import.
- No hosted Cursor plugin/marketplace slash surface.
- No PARITY `Matched` for slash/pickers/keymap without separate clean TUI fixtures.
- No oracle/reference edits; no Cursor source copied into `src/`.

## Evidence and Current Behavior

### Cursor observations (2026-09-17)

Sources: `agent-local --help`; bundle
`~/.local/share/cursor-agent-local/versions/2026.09.15-d2fe57e/{index.js,7076.index.js,8366.index.js}`;
disposable `~/.cursor/cli-config.json`; headless `mcp list`.

- CLI subcommands (Observed help): persist, login/logout, mcp, plugin, worker,
  status|whoami, models, about, update, create-chat, generate-rule, agent,
  ls, resume, shell-integration install/uninstall.
- TUI slash strings (bundle, not full registry): `/model`, `/config`, `/mcp`,
  `/hooks`, `/term`, `/plan`, `/worktree`, `/theme`, `/usage`, `/new`,
  `/resume`, `/login`, `/logout`, `/btw`, `/run-everything`.
- Config: `modelSlashCommands: true`, `editor.vimMode`, `hints: true`,
  `display.mode: "zen"`, `steering`/`rewind` booleans.
- Hints in bundle: Ctrl+J newline, Ctrl+S save, Ctrl+D/C exit patterns.
- Headless `mcp list`: "No MCP servers configured (expected in .cursor/mcp.json
  or ~/.cursor/mcp.json)" — no Cursor login required.

### `ca` source/snapshot facts

- `src/tui/commands.rs` — centralized `CommandId` (~106 variants), specs with
  aliases/usage/category/mutation; palette and discovery snapshots
  (`command_palette`, `command_discovery` "Commands 1/106").
- Tested parse examples include `/model`, `/m`, `/sessions`, `/keymap`,
  `/provider`, `/btw`, `/sandbox`, `/permissions`, `/plan`, etc.
- `src/keymap.rs` — typed chords, reserved emergency/composer/navigation keys,
  `/keymap` manager snapshots (`keymap_capture`, `keymap_manager`, …) with
  Fixed lines (Ctrl+R Diff, Ctrl+C cancel/quit).
- PARITY: slash commands/pickers row `Unverified`; `/keymap` and `/provider`
  documented as CA extensions.

### Inferences (labeled)

- Inferred: CA command surface is already broader than Cursor TUI slash names
  observed; work is hygiene/alias/docs, not parity catch-up.
- Inferred: Model-as-slash must not bypass model validation, profile scoping,
  or `/model` confirmation semantics.

## Scope

Allowed files (when activated):

- `src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs` (palette/help only)
- `src/tui/composer.rs` if slash completion needs alias insertion
- `src/keymap.rs` / keymap help text only if needed for Fixed/custom clarity
- Focused tests and snapshots
- `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, this plan

Forbidden files/areas:

- `oracles/`, `reference/`, Cursor install trees
- Permission/sandbox/approval policy modules except read-only citations
- Credentials; user runtime state in tests (temp homes only)
- Unrelated active ExecPlan hunks (AUT-231, SEC-158, CUR-245, SEC-246, TUI-247)

Runtime state affected: disposable test homes; no install unless separately authorized.

## Design and Invariants

1. **Registry is authority** — New aliases only via CommandId/spec or an
   explicit, tested alias table; no hidden stringly-typed commands.
2. **Model-as-slash subordinate** — If implemented: token must resolve to a
   known configured/documented model id; same effects as `/model <id>`; never
   invents providers or secrets; never auto-yolo.
3. **Palette completeness** — Every visible command appears in discovery/help
   with usage when it has user-visible effects.
4. **Keymap Fixed vs custom** — Reserved keys remain non-remappable; capture
   UI continues to show Fixed vs custom distinction.
5. **Evidence discipline** — Cursor help/strings inform naming only; CA
   snapshots/tests define acceptance.
6. **No authority widening** — Slash UX never bypasses GRD approval boundaries.

## Milestones

### M1 — Slash inventory characterization

- Generate or test-assert: count of visible CommandIds vs palette/discovery
  snapshot coverage; list any visible command missing usage/help.
- Validation: focused `commands.rs` / render tests.

### M2 — Optional model-as-slash alias

- Only if M1/user still want it: implement narrow alias for exact known model
  ids when they do not collide with command names; delegate to existing
  `/model` path.
- Failure-first tests: unknown id, collision with command, `--once` semantics,
  provider scoping.
- Validation: focused input/commands tests.

### M3 — Palette/help polish

- Fix missing usage lines or discovery ordering issues found in M1.
- Update command_discovery/palette snapshots intentionally.
- Validation: render focused tests; zero `.snap.new`.

### M4 — PARITY Observed notes (CLI only)

- Record Observed: Cursor CLI help command list; `mcp list` empty-state
  citation with version `2026.09.15-d2fe57e` and capture date.
- Leave TUI slash/picker/keymap rows Unverified.
- Validation: docs; `oracles/` status empty.

## Acceptance Criteria

- [x] M1 inventory exists in plan Progress; no visible command lacks help/usage without an explicit exception list.
- [x] If model-as-slash ships: collisions and unknown ids tested; behavior equals `/model` for known ids.
- [x] Keymap Fixed/custom presentation unchanged or snapshot-updated deliberately.
- [x] No permission/sandbox default changes; no Cursor autorun defaults.
- [x] No oracle/reference edits; no Cursor source in `src/`.
- [x] Focused tests + fmt + scoped clippy + snapshot hygiene recorded.

## Validation

Focused: commands registry tests; palette/discovery render tests; keymap help tests if touched.

Completion (when implementation milestones run): `TESTING.md` gates for touched surface; fmt; clippy; zero `.snap.new`; clean `oracles/`/`reference/` status.

Manual/visual: interactive `/` palette filter, `/keymap`, `/model` in a disposable workspace.

Blocked checks: Cursor interactive picker/keymap TUI capture; full PARITY Matched for slash surface.

## Progress

- [x] Slash/pickers/keymap study recorded (2026-09-17).
- [x] This DRAFT ExecPlan written for later activation.
- [x] M1 slash inventory characterization
- [x] M2 model-as-slash alias (optional)
- [x] M3 palette/help polish
- [x] M4 PARITY Observed CLI notes
- [x] Explicit activation and completion

## Decisions

- Decision: Status DRAFT until the user activates this plan.
  Evidence: User asked for CMD-248 drafted the same way.
  Consequence: No `src/` edits from this document alone.

- Decision: Do not reduce CA slash surface toward Cursor's observed set.
  Evidence: CA registry (~106 commands) is a product strength; Cursor TUI
  slash names observed are a subset/different product.
  Consequence: Scope is hygiene and optional aliasing only.

## Surprises and Discoveries

- Cursor TUI slash registry is not cleanly listable from minified bundles;
  CA's typed registry is better evidence for its own surface.
- Cursor `modelSlashCommands` is the only picker/command idea clearly novel
  versus what CA already ships at depth.

## Recovery and Rollback

- DRAFT abandon: no source rollback.
- Implementation: revert only Scope files via normal Git; review snapshot diffs.

## Completion Record

Changed files:
- `src/tui/commands.rs` — `parse_command_line_with_model_alias`; `/tasks` usage; visible-command description test
- `src/tui/input.rs` — slash submit uses model-alias resolver when token matches `model_list`
- `PARITY.md` — Cursor CLI Observed notes (mode/plan, tool_call, session resume)

Commands and results:
- Focused tests PASS: every_visible_command_has_nonempty_description (+ /tasks usage); model_alias_becomes_model_command_and_commands_win
- Registry breadth preserved (106 visible commands); explicit commands win over model aliases
- fmt + clippy PASS

Remaining risks: model alias only for bare tokens that exactly match configured model ids.
