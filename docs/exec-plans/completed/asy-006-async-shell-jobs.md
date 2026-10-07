# ASY-006 — Asynchronous Shell and Background Jobs

Status: COMPLETE
Owner/session: current Codex roadmap implementation
Last updated: 2026-08-09

## Goal

Keep terminal event polling responsive while foreground and background shell jobs stream bounded output, accept stdin, support cancellation, and shut down with `ca`.

## Delivered

- `JobManager`, UUID `JobId`, `JobStatus`, `JobInfo`, and channel-delivered `JobEvent` lifecycle.
- Independently bounded stdout/stderr readers, stdin writes, configured timeout, cancellation, output snapshots, and process-group cleanup.
- Responsive direct `!command`, silent `!!command`, and process-bound background `!& command` execution with no nested runtime calls.
- Optional background `run_command` plus `await_job`, `write_shell_stdin`, and `kill_job` tool schemas/execution.
- `/jobs` list/detail UI showing command, foreground/background mode, status, elapsed time, recent/full output, scrolling, cancel, and stdin entry.
- Central normal-exit shutdown across TUI, headless, and scripted execution.

## Validation

- Seven job lifecycle/responsiveness tests: PASS.
- Seven tool/parser/approval/input tests: PASS.
- Four reviewed jobs list/detail snapshots: PASS; no `.snap.new` files remain.
- `cargo test`: PASS, 157 unit plus 3 integration tests.
- `cargo fmt --check`: PASS.
- Strict Clippy: only the same 3 pre-existing diagnostics outside ASY scope.
- Release build, help/version smoke, nested-runtime search, and live PID check: PASS.

## Boundaries Preserved

Background mode retains approval classification. Jobs live only inside the current `ca` process; no detached daemon or persistent runtime state was introduced. Oracle/reference trees and the existing live TUI were not modified or restarted.
