# Roles Subsystem

Owns AI role runners and compact context packs. Rust routing decides whether a role is called; role code should return decisions, not mutate queue state or commit work directly.

Depends on: prompts/schemas assets and Codex runner seam; authority stays in `app`.

Read this first for CEO, Planner, Worker, Senior review, or Senior repair prompt assembly boundaries.

Do not read for queue mutation, git commit staging, or verification spawn.

Note: much live role prompt building still resides in `src/app.rs` while this module documents intended ownership.

Tests: role routing via fake runner filters (see `TESTING.md`).
