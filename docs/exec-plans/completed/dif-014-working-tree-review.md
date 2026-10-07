# DIF-014 — Full Working-Tree Change Review

Status: COMPLETE
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Replace diff-stat status messages with a read-only full-screen working-tree review for tracked and untracked change metadata and per-file unified diffs.

## Delivered

- `GitChangeService` invokes Git asynchronously with argument arrays and parses NUL-delimited porcelain status.
- The shared `/diff`, `/changes`, and Ctrl+R action opens a full-screen “Working tree changes” surface.
- List entries show staged/unstaged state, status, rename paths, and textual line counts.
- Enter loads sanitized staged and/or unstaged unified diffs for tracked files; untracked contents are rejected.
- Esc returns from diff to list and then closes the surface; scrolling works at narrow and wide widths.

## Validation

- `cargo test changes::`: PASS, 4 service tests.
- Three change-review input tests: PASS.
- Four reviewed wide/narrow snapshots: PASS; no `.snap.new` files remain.
- `cargo test`: PASS, 142 unit plus 3 integration tests.
- Targeted rustfmt and repository `cargo fmt --check`: PASS.
- Strict clippy: only the same 3 pre-existing diagnostics outside DIF scope.
- Release build and local help/version smoke: PASS.
- Existing live TUI PID 51440 remained running and was not restarted or interrupted.

## Boundaries Preserved

No oracle/reference file, runtime configuration, user session, or production working-tree file was modified by the review service. The surface contains no stage, revert, delete, or rewrite operation.
