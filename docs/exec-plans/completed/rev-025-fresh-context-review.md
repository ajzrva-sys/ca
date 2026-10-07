# REV-025 — Fresh-context Model Review

Status: COMPLETE
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Add a read-only `/review` workflow that builds a bounded argument-safe Git scope, runs a model in a fresh isolated context with only project instructions, diff, file/Git reads, and a structured finding reporter, persists the review record, and appends a concise canonical follow-up exchange.

## Non-Goals

- AI walkthroughs, mutation/staging/revert, untracked contents, chat/checkpoint/plan/skill/MCP/agent context, or Cursor parity claims.

## Evidence and Current Behavior

- `src/changes.rs::GitChangeService` already uses argument arrays for working-tree status, numstat, and per-file diffs while withholding untracked contents.
- `/diff` owns the non-model “Working tree changes” viewer and must remain unchanged.
- `SessionV3` has `review_records`; the typed registry reserves `ToolProfile::Review` but lacks an internal structured finding reporter.
- Provider stream assembly is shared and typed; `ChatBackend` gives tests an injectable no-network provider boundary.

## Scope

Allowed: `src/`, temporary-repository/fake-provider tests, review snapshots, this plan, and harness docs.
Forbidden: oracle/reference trees, credentials, user runtime state, installed binaries, and the live TUI/process.

## Design and Invariants

- Targets: working, base branch, commit SHA, or custom instructions over working changes. Refs are validated before argument-safe Git invocation.
- Input is assembled per file, capped at 1 MiB only between complete file sections; untracked paths are listed but never read.
- Fresh messages contain project instructions plus a clearly delimited review scope/diff. They exclude canonical chat, checkpoint, work state, skills, MCP, and subagents.
- Review profile exposes read-only file/Git access and internal `report_review_findings`; it cannot mutate.
- Findings are typed severity/title/rationale/file/line range/confidence/suggested validation and terminal-sanitized.
- Default provider/model follows current selection; optional review provider/model config overrides it explicitly.
- Persist a `ReviewRecord` and recorded review evidence event, then append one concise user/assistant review exchange to canonical history for follow-up.

## Milestones

### M1 — Git scopes and privacy characterization
- Add dirty temporary repositories for working/base/commit/custom, unsafe refs, Unicode, truncation, and untracked privacy.

### M2 — Injectable structured reviewer
- Add internal reporting schema/parser, isolated prompt/messages, fake-provider tests, and deterministic failures.

### M3 — Orchestration, persistence, and TUI
- Add `/review`, async lifecycle, optional model config, review record/evidence/history updates, and wide/narrow findings snapshots.

### M4 — Completion
- Run focused/full gates, reconcile docs, and move this plan.

## Acceptance Criteria

- Every target uses safe Git arguments and unsafe refs fail before execution.
- Input never exceeds 1 MiB, never splits a file section, and never includes untracked contents.
- Fake-provider captures no chat/checkpoint/plan/skill/MCP/agent data and returns typed findings through the sole reporter.
- Review profile cannot mutate; successful reviews persist records/evidence and concise history.
- `/diff` remains the existing non-model viewer.

## Validation

Focused: PASS — temporary Git scopes/privacy/cap tests, fake-provider isolated-context/reporting tests, registry/command/provider-selection/session tests.
Completion: PASS — 281 unit tests plus 18 CLI tests, format, strict Clippy, release build, and local help/version.
Manual/visual: PASS through 31 render tests and 58 accepted snapshots, including reviewed 100×30 and 60×24 review output; no `.snap.new` files remain. The live TUI was intentionally not touched.
Blocked: live-provider review and clean Cursor comparison require external/runtime access and remain unclaimed.

## Progress

- [x] Git service, provider, registry, session, command, and render boundaries traced.
- [x] Git target/privacy tests and service complete.
- [x] Injectable reviewer complete.
- [x] TUI/persistence integration complete.
- [x] Completion gates/docs complete.

## Decisions

- Decision: cap at complete per-file diff sections rather than byte-truncating one monolithic Git output.
  Evidence: partial patches are ambiguous and can split headers/hunks; the requirement explicitly caps on file boundaries.
  Consequence: truncated reviews identify omitted file names without including partial content.

## Surprises and Discoveries

- `git diff HEAD -- <path>` provides one argument-safe combined staged/unstaged working-tree patch, so the model input does not need to concatenate overlapping staged and unstaged hunks.
- The repository does not include `cargo-insta`; snapshots were reviewed from `.snap.new` output and accepted through explicit repository patches.

## Recovery and Rollback

Use milestone-scoped `apply_patch`; all tests use temporary repositories/providers and never mutate the working repository.

## Completion Record

Changed files: `src/review.rs`, `src/changes.rs`, `src/main.rs`, `src/app.rs`, `src/config.rs`, `src/work.rs`, `src/session.rs`, `src/agent/{registry,tools}.rs`, `src/tui/{commands,input,render_tests}.rs`, two review snapshots, and harness/plan docs.

Commands/results:

- focused review/Git/registry/command/session/provider tests: PASS;
- `cargo test tui::render_tests -- --nocapture`: PASS, 31 tests / 58 snapshots;
- `cargo test --no-fail-fast`: PASS, 281 unit + 18 CLI;
- `cargo fmt --check`: PASS;
- `cargo clippy --all-targets -- -D warnings`: PASS;
- `cargo build --release`: PASS;
- `target/release/ca --help` and `--version`: PASS (`ca 0.1.0`).

Remaining risks: the provider review was validated with an injected fake backend, not live credentials; no Cursor parity claim is made because `PAR-005` remains blocked.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record the review boundary and final evidence.
