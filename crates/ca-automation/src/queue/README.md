# Queue Subsystem

Owns NEXT_STEPS parsing, dependency gates, non-AI queues, and tracking document patches. It should depend on domain types and pure text helpers, not on Codex process execution.

Depends on: `domain` task/policy types; no process spawn.

Read this first for task-card parsing, exact task-ID section bounds, queue promotion, or status-note rendering.

Do not read for verification execution, guard snapshots, or role prompts.

Tests: `cargo test queue::next_steps`.
