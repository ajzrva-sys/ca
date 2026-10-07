# CMD-010 — Unified Searchable Command Registry

Status: COMPLETE (2026-08-09)
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Make one typed registry authoritative for slash-command lookup, aliases, help, palette search, availability, and orchestration dispatch.

## Non-Goals

- Do not redesign individual command behavior.
- Do not expose the unsafe legacy `/commit` command in the palette.
- Do not implement queue, timeline, context, diff, or job features owned by later roadmap tasks.

## Evidence and Previous Behavior

- `src/tui/input.rs::execute_slash` recognized substantially more commands than the hard-coded palette in `src/tui/render.rs`.
- Palette Up/Down handling was empty and unknown strings silently became manual model IDs.
- Several commands performed synchronous side effects inside input handling.

## Scope

Allowed files: command/input/render/app/main modules, focused tests/snapshots, and harness docs.
Forbidden: `oracles/`, `reference/`, runtime config/session files, unrelated refactors.
Runtime state affected: none during tests.

## Design and Invariants

- `CommandSpec` is the only source of command metadata.
- Input handling emits `InputAction`; orchestration performs effects.
- Hidden commands remain resolvable by exact name but never appear in help or palette.
- Busy-unavailable commands fail with a status message rather than executing.

## Completed Milestones

### M1 — Registry and parser

- Added typed command IDs/specs/actions and uniqueness/alias/parser tests.

### M2 — Palette and dispatch

- Registry now drives filtering, selection, help, and dispatch.
- Moved Git, filesystem, config, session, clipboard, editor, and process side effects out of the input reducer.

### M3 — Validation and handoff

- Focused registry/input/render tests pass.
- Two wide/narrow command-palette snapshots were reviewed and accepted.
- Full Cargo tests and release build pass; baseline format/lint failures are recorded below.

## Acceptance Results

- Every prior slash command resolves through the registry.
- Search matches canonical names, aliases, and descriptions.
- Up/Down/Enter operate on the filtered list at wide and narrow sizes.
- `/commit` is hidden from discovery and disabled on exact invocation pending approval-safe redesign.
- Input handling emits typed actions and does not execute process, Git, config, session, clipboard, or editor side effects.

## Validation Evidence

- `cargo test tui::commands --no-fail-fast`: PASS, 4 tests.
- `cargo test slash_palette --no-fail-fast`: PASS, 2 tests.
- `cargo test idle_only_palette_commands --no-fail-fast`: PASS, 1 test.
- `cargo test command_palette --no-fail-fast`: PASS, 2 snapshots.
- `cargo test`: PASS, 105 tests.
- `rustfmt --edition 2021 --check src/tui/commands.rs`: PASS.
- `cargo fmt --check`: FAIL on pre-existing repository-wide drift.
- `cargo clippy --all-targets -- -D warnings`: FAIL on the same three pre-existing diagnostics in `src/agent/tools.rs` and `src/approval.rs`.
- `cargo build --release`: PASS.
- `./target/release/ca --version` and `--help`: PASS.
- Existing live TUI PID 51440 remained running; no manual live-provider parity claim was made.

## Recovery and Rollback

Revert only CMD-010 files/blocks; do not use destructive Git commands or touch the live TUI process.
