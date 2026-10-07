# Guard Subsystem

Owns scope checks: repository snapshots, changed files, allowed/read-only globs, diff summaries, and guard reports.

Depends on: path layout and config ignored/allowed patterns; not on Codex.

Read this first for file-scope violations or risk inputs derived from changed files.

Do not read for verification command execution or Markdown queue parsing.

Note: `.orchestrator/**` is typically ignored by guard snapshots; separate control-plane checks in `app` cover orchestrator authority files.

Tests: focused guard/diff tests under `cargo test` (see `TESTING.md`).
