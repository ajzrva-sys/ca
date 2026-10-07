# Domain Subsystem

Owns shared vocabulary: `TaskSpec`, policy enums, events, route regret, route-learning scorecards, derived status, acceptance contracts, and tool-health classifications.

Depends on: serde/std only. No file IO, process execution, Codex calls, or terminal rendering.

Read when changing persisted type shapes, advisory telemetry vocabulary, or Worker acceptance contract classification.

Do not read for queue Markdown parsing, verification spawn, or cycle routing (those stay in `queue` / `verification` / `app`).

Tests: `cargo test acceptance_contract`, `cargo test tool_health`, `cargo test derived_status`, `cargo test route_learning`, `cargo test route_regret`.
