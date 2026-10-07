# Codex Adapter Subsystem

Owns model process invocation, JSONL stream parsing, live rendering boundaries, and prompt/cost accounting edges. It should be an adapter, not a policy engine.

Optional local Ollama / Apple MLX LM role calls live in `src/providers/` (HTTP `/v1/chat/completions`). Codex spawning stays here.

Depends on: paths, telemetry helpers; policy stays in `app` / `roles`.

Read this first for `codex exec` spawning, process-group kill on timeout, or stream parsing.

Do not read for task parsing, guard scope, verification recipes, or Ollama HTTP (use `src/providers/`).

Current ownership:

- `run.rs` owns role-call `codex exec` spawning, stdin prompt delivery, run artifacts, AI escalation events, and model-call cost records.
- `stream.rs` owns pure JSONL parsing helpers for final agent messages, command executions, and changed files.
- `prompt_pack.rs` owns compact ContextPack receipts and section budgets.
- `live.rs` is still a target home. Live terminal rendering remains mostly in `src/app.rs` until it can move without changing presentation behavior.

Tests: `cargo test codex`, `cargo test parse_json_response`, `cargo test ollama`.
