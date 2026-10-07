# ORCH-INTEGRITY-001: Authority, Queue Truth, And Commit Integrity

## Purpose / Big Picture
Seal Rust authority around queue identity, command validation, acceptance/commits, process timeouts, and CLI locks so unattended and Hermes-supervised use cannot corrupt backlog truth or commit unverified trees.

## Progress
- [x] Exact task-ID section matching and single Status mutation
- [x] Repair CHURN-002 dual Status / stale Done claim
- [x] Advisory role read-only sandboxes + control-plane mutation detection
- [x] ValidatedCommand / cwd containment / dangerous arg denial
- [x] Process-group timeouts for Codex and verification
- [x] Acceptance transaction: commit verified tree before Done finalize
- [x] ORCH-LEARN-002 lock classification for mutating CLI commands
- [x] Strict schema tests + Senior prompt consistency
- [x] CI workflow (GitHub Actions GRD-001)
- [x] CHURN-002 context delta + verification_improved
- [x] CHURN-003 lease context foundation
- [x] Module extraction: verification DTOs, Config/State, git::find_git_root
- [x] Hermes bridge v1 seams (`step --json` / `status --json` + `docs/HERMES.md`)

## Surprises & Discoveries
- Control-plane diff must ignore Rust-refreshed Worker prompt/schema seeds and runtime lock files.
- Non-TTY `step_once` must stop on human disposition instead of blocking on stdin (Hermes/unattended).

## Decision Log
- Exact task-ID matching; Status mutation normalizes to one `- Status:` line.
- Accepted Worker commits stage guard-changed files plus queue/tracking docs only.
- Advisory roles default to `read-only`; Worker remains `workspace-write`.
- Hermes Phase 1 is CLI JSON receipts; MCP deferred.

## Validation and Acceptance
- Passed: `cargo fmt -- --check`
- Passed: `cargo test` (262 lib tests)
- Passed: `cargo test role_schemas_require`
- Passed: `cargo test lease`
- Passed: `cargo test lock` (via full suite)

## Outcomes & Retrospective
Integrity plan implemented. Remaining follow-ons: clippy `-D warnings` cleanup, load/save and live-render extraction, Hermes MCP.
