# Verification Subsystem

Owns deterministic recipes, legacy command parsing, allowlist validation, ValidatedCommand process execution (cwd containment, process-group timeouts), evidence/plan DTOs, and diagnostic summaries. It is the local CI/evidence machine.

Depends on: `domain` / `repair` vocabulary; not on role runners or CLI parsing.

Read this first for command validation, failure classification, evidence DTOs, or runner timeouts.

Do not read for queue Markdown or live terminal UI (plan construction / JSONL persistence orchestration still partly live in `app`).

Tests: `cargo test verification`, `cargo test recipe`, `cargo test legacy_intern_support_contract`.
