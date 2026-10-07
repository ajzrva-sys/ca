# Context Subsystem

Owns bounded Worker file leases and a stale-tolerant repo-hint cache. Rust decides which file bodies enter Worker prompts; Workers may request `expand_lease`, not global repo scans.

Depends on: filesystem reads for leased paths only; orchestration wiring remains in `app`.

Read when changing lease caps, path safety, or how leased contents render into context packs.

Do not read for queue parsing, verification recipes, or Codex spawning.

Tests: `cargo test lease`.
