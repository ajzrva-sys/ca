# ca Product Roadmap

> Maintained direction only. Source and `CONTINUITY.md` own current behavior,
> `NEXT_STEPS.md` owns executable work, and `PARITY.md` owns oracle claims.
> Avoid hand-counted tools and inferred parity here.

## Current priorities

1. Reconstruct a tracked, clean, reproducible `ca`/`codex-team` release source
   with pinned root validation and deterministic readiness receipts.
2. Require host-owned verification receipts and monotonic budgets before agent
   graph dependencies can be released.
3. Complete the cancellation-safe exact sudo supervisor before adding encrypted
   authenticated WebSocket service handoff.
4. Fix Markdown fidelity, focused-surface routing, and native terminal safety
   before adding more TUI breadth.
5. Finish the active reviewed-learning and delegation pilots, then add
   evidence/confidence decay, human-feedback receipts, and held-out skill evals.
6. Improve skill/command discovery and Mission Control without silently
   activating capabilities.
7. Pursue shared-runtime/performance work and capture clean versioned parity
   fixtures; every current Cursor parity row remains `Unverified`.

## Historical 24-tool snapshot (superseded)

The sections below are preserved only as early product history. They are not a
current capability matrix, configuration guide, or task queue.

| Tool | Status |
|------|--------|
| `read_file` | ✅ |
| `write_file` | ✅ |
| `edit_file` | ✅ |
| `apply_patch` | ✅ |
| `run_command` | ✅ |
| `list_dir` | ✅ |
| `search_files` | ✅ |
| `glob` | ✅ |
| `delete_file` | ✅ |
| `web_search` | ✅ |
| `web_fetch` | ✅ |
| `read_lints` | ✅ |
| `read_todos` | ✅ |
| `create_plan` | ✅ |
| `ask_question` | ✅ |
| `update_todos` | ✅ — complete/add TODO markers |
| `create_directory` | ✅ — mkdir -p |
| `switch_mode` | ✅ — agent/plan/ask/debug via marker |
| `send_message` | ✅ — inline progress updates |
| `fetch` | ✅ — download file from URL |
| `blame` | ✅ — git blame with line ranges |
| `switch_branch` | ✅ — git checkout |
| `generate_image` | ✅ — IMAGE_GEN_URL/IMAGE_GEN_API_KEY |
| `task` | ✅ — spawn subagent (explore/plan/general/custom) |

## Subagent System

- ✅ Built-in agents: explore (read-only), plan (read-only), general (full access)
- ✅ Custom agents from `.ca/agents/*.md` with YAML frontmatter
- ✅ `task` tool spawns subagent with filtered tool set
- ✅ Subagent returns final answer to parent agent

## UX Features

- ✅ `/undo` — revert last file write/edit/delete
- ✅ Context compression — auto-summarize when >50k chars
- ✅ Token usage display — status bar shows session token count
- ✅ Shell history — `!` commands persisted to `~/.config/ca/shell_history`
- ✅ `@file` mentions — already working via expand_file_mentions
- ✅ Model cycling — `Ctrl+P` cycles models, `Ctrl+L` opens picker
- ✅ External editor — `Ctrl+G` opens $EDITOR
- ✅ Kill ring — `Ctrl+K`/`Ctrl+Y`/`Alt+Y`
- ✅ Message queue — `Alt+Enter` queues messages

## Historical exclusions (superseded)

| Feature | Why |
|---------|-----|
| MCP tools | No MCP server ecosystem for local tool |
| GitHub PR tools | Use `gh` CLI directly |
| VSCode extension | CLI-first tool |
| Sound notification | User declined |
| Git diff on startup | User declined |
| Image paste | CLI-only, no clipboard integration |

## Key Configuration

```toml
# ~/.config/ca/config.toml
[providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_key = "${DEEPSEEK_API_KEY}"

[providers.ofox]
base_url = "http://127.0.0.1:11434/v1"
api_key = "ollama"
models = ["qwen2.5:14b"]

[providers.mimo]
base_url = "https://api.xiaomi.com/v1"
api_key = "${XIAOMI_API_KEY}"
models = ["mimo-v2.5"]

[providers.ollama]
base_url = "http://127.0.0.1:11434/v1"
api_key = "ollama"
models = ["deepseek-coder-v2:16b"]
```

## Key Paths

- Binary: `~/.local/bin/ca`
- Config: `~/.config/ca/config.toml`
- Shell history: `~/.config/ca/shell_history`
- Image output: `.ca-images/`
- Custom agents: `.ca/agents/*.md`
- Sessions: `.ca-sessions/`
