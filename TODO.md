# ca — Cursor Agent TUI Clone: Remaining Work

> Historical backlog snapshot. It predates much of the current implementation. Use `NEXT_STEPS.md` for executable work; consult this file only as discovery evidence and verify claims in source.

## Context

`ca` is a Rust CLI at `~/project/ca/` that clones the Cursor Agent interactive TUI.
It connects to local LLM APIs (Ollama, MLX, MiMo, ofox, etc.) via OpenAI-compatible `/chat/completions`.
Binary installed at `~/.local/bin/ca`. Config at `~/.config/ca/config.toml`.

**Currently working:**
- Headless mode (`echo "prompt" | ca`) — verified end-to-end with Ollama qwen2.5:0.5b
- TUI launches, renders title bar, input, conversation, status, model picker, slash palette
- Streaming responses via tokio select! (non-blocking event loop)
- Ctrl+C cancel mid-stream
- Markdown rendering (bold, italic, headings, inline code, code blocks, lists, links, rules)
- Cursor movement (left/right/home/end/Ctrl+A/E), history (up/down), word delete (Ctrl+W), kill line (Ctrl+U)
- Model picker (Ctrl+K) with fuzzy filtering
- Slash commands (/m, /p, /a, /d, /y, /ar, /c, /q, /h)
- Multiline input (Shift+Enter, Ctrl+J)
- Auto-scroll to bottom during streaming; PageUp/Down manual scroll

**Architecture:**
- `src/main.rs` — tokio runtime, select! loop, dispatch to LLM
- `src/app.rs` — AppState (all UI state), Message, ToolCall, ModelInfo structs
- `src/config.rs` — TOML config loading with ${ENV_VAR} expansion, ~/.config/ca/config.toml
- `src/tui/event.rs` — crossterm terminal setup, async event polling
- `src/tui/input.rs` — keybind handling, model picker/slash palette logic
- `src/tui/render.rs` — ratatui rendering (title, conversation, input, status, overlays)
- `src/markdown.rs` — pulldown-cmark to ratatui Line/Span conversion
- `src/agent/client.rs` — OpenAI-compat streaming HTTP client (reqwest + SSE parsing)
- `src/agent/loop_.rs` — unused agent loop scaffolding (tool execution, multi-turn) — can be deleted or completed

**Key dependencies:** ratatui 0.29, crossterm 0.28, tokio, reqwest, serde, pulldown-cmark 0.12

**Build:** `cd ~/project/ca && cargo build --release && cp target/release/ca ~/.local/bin/ca`

---

## Priority 1: Tool Execution (the real agent loop)

Cursor Agent isn't just chat — it reads/writes files, runs commands, edits code. Without this it's just a chat TUI.

### 1.1 Implement tool definitions
- [ ] In `src/agent/client.rs`, expand `default_tools()` to return real tool schemas:
  - `read_file(path)` — read file contents
  - `write_file(path, content)` — write/overwrite file
  - `list_dir(path)` — list directory contents
  - `run_command(command, cwd)` — execute shell command (non-interactive, capture stdout)
  - `search_files(pattern, path)` — grep-like search
  - `edit_file(path, old_string, new_string)` — targeted find-and-replace in files
  - `apply_patch(path, patch)` — unified diff application
- Each tool needs a proper JSON Schema `parameters` object in the ToolDef

### 1.2 Implement tool execution
- [ ] In `src/main.rs`, when LLM returns tool_calls, execute them locally:
  - File operations via `tokio::fs`
  - Shell commands via `tokio::process::Command` with timeout (30s default, configurable)
  - Send tool output back as a `role: "tool"` message and continue the agent loop
- [ ] The agent loop should support multi-turn: LLM calls tool → execute → feed result back → LLM continues, up to `agent.max_turns` from config (default 100)
- [ ] **Approval modes matter:**
  - `allowlist` (default): prompt user before executing "dangerous" tools (write_file, run_command) — show the command/file and ask [y/n/e]
  - `auto-review`: auto-approve safe tools (read_file, list_dir, search_files), prompt for destructive ones
  - `yolo` (RunEverything): auto-approve everything
- [ ] Approval prompt should render inline in the conversation, accept key press (y/n/e/d where e=edit, d=deny)

### 1.3 Working directory awareness
- [ ] Agent should know current working directory (the directory where `ca` was launched)
- [ ] Show CWD in title bar or status bar
- [ ] `list_dir` and file tools default to CWD
- [ ] Allow user to @-mention files by path to add them to context

---

## Priority 2: Context & Context Management

### 2.1 System prompt
- [ ] Expand the system prompt beyond "You are a helpful coding assistant":
  - Tell the model about the working directory, platform (macOS), shell, available tools
  - Instruct it to use tools proactively for code tasks
  - Include concise coding style preferences

### 2.2 Context window management
- [ ] Track token count (estimate from message lengths: ~4 chars/token)
- [ ] When approaching model's context limit, auto-compress old messages:
  - Summarize or truncate tool outputs longer than N lines (default 2000 chars)
  - Keep last N messages intact, summarize earlier turns
- [ ] Show token count in status bar

### 2.3 File context
- [ ] Support `@filename` in input to include file contents in the prompt
- [ ] Support `@directory` to include directory tree
- Parse `@` mentions from input before sending, inject file contents as context

---

## Priority 3: TUI Polish & Cursor Parity

### 3.1 Better input area
- [ ] Support multi-line input display properly (currently input area is 1 line tall)
  - Input box should grow/shrink up to N lines (max 8)
  - When input is multiline, shift conversation up
- [ ] Line wrapping in input for long lines
- [ ] Word-level backspace (Ctrl+W) — partially done, verify works
- [ ] Paste support (bracketed paste mode) — crossterm supports this, enable it

### 3.2 Conversation improvements
- [ ] **Diff rendering**: when tool edits files, show unified diffs with green/red coloring (like Cursor does)
- [ ] Code block syntax highlighting: for code blocks in assistant responses, apply basic syntax coloring
  - Use a simple approach: highlight keywords, strings, comments for common languages
  - Don't add syntect back (too heavy) — do simple regex-based highlighting for Rust/Python/JS/Go/Bash
- [ ] Smooth scrolling: currently scroll jumps by lines, that's fine
- [ ] Message timestamps on hover? No — keep it simple like cursor
- [ ] Copy support: select text and copy (OS-level terminal select works, but add Ctrl+O to copy last code block)
- [ ] Token count per message shown in dim text

### 3.3 Streaming cursor
- [ ] The `▌` cursor at end of streaming should blink (toggle on/off each 500ms during streaming)
- Requires a tick/render timer — use the existing Event::Tick to toggle

### 3.4 Model picker improvements
- [ ] Filter should be case-insensitive (already done)
- [ ] Show model details: provider, context window (if known)
- [ ] Allow adding custom model inline (type model name that doesn't exist → use it)
- [ ] Group models by provider with headers
- [ ] After selecting model, persist choice to config file

### 3.5 Welcome screen improvements
- [ ] When in a git repo, show repo name/branch
- [ ] Recent files or sessions
- [ ] Example prompts: "Explain this codebase", "Find bugs in...", "Write tests for..."

### 3.6 Tab cycle mode indicator
- Currently Tab cycles agent/plan/ask/debug but the UI just shows text in title
- Add visual feedback: brief flash/toast when mode changes (status message already does this, verify visible)

---

## Priority 4: Persistence & Sessions

### 4.1 Conversation persistence
- [ ] Save conversations to `~/.local/share/ca/sessions/` as JSON files
- [ ] Auto-save every message (so crash doesn't lose history)
- [ ] `/resume` command to list and resume past sessions

### 4.2 Config editing
- [ ] `/config` command opens config in $EDITOR
- [ ] `/provider` command to list/add/remove providers
- [ ] Add provider for MiMo, ofox, deepseek in default config (already done in config.rs, verify)

---

## Priority 5: Keybind Refinements

Cursor-agent specific keybinds to match:
- [ ] `Ctrl+R` — review changed files (show git diff)
- [ ] `Ctrl+O` — expand/collapse long content (code blocks, tool outputs)
- [ ] `Ctrl+P` — same as Ctrl+K (model picker) — some versions use this
- [ ] `Esc` then `Esc` — quick quit (double-tap Esc to exit)
- [ ] `Ctrl+B` twice — send to background (detach, keep running)
- [ ] `↑` when input empty — edit previous message, don't just navigate history
- [ ] `Ctrl+Z` — undo last edit (if we support editing)

Input box keybinds:
- [ ] `Alt+B` / `Alt+F` — word left/right
- [ ] `Ctrl+K` — delete from cursor to end of line (note: this conflicts with model picker! In cursor, Ctrl+K is model picker globally, but in input it's kill-to-end. Resolve: when input focused and non-empty, Ctrl+K kills; when empty, opens model picker. Or use different key.)
- [ ] `Ctrl+Y` — paste from kill ring (after Ctrl+K)

---

## Priority 6: Code Quality / Cleanup

### 6.1 Warnings
- [ ] Fix all 27 compiler warnings:
  - Remove dead code in `agent/loop_.rs` (either complete the agent loop there or delete it)
  - Remove unused imports
  - Rename unused variables with `_` prefix
  - Run `cargo fix --bin "ca" -p ca` to auto-fix the 16 machine-fixable suggestions

### 6.2 Module structure
- [ ] `agent/loop_.rs` has `execute_turn`, `build_system_prompt`, `is_safe_tool`, `execute_tool` — these are unused. Either:
  - Complete the multi-turn agent loop properly in loop_.rs and call it from main.rs, OR
  - Delete loop_.rs entirely and move any needed functions into main.rs or client.rs
- [ ] Extract tool execution into `src/agent/tools.rs` (separate file from client.rs)

### 6.3 Error handling
- [ ] Replace `.unwrap()` calls with proper `?` error propagation
- [ ] Add graceful error messages for: API key missing, network down, model not found, rate limited
- [ ] If Ollama isn't running, show clear error "Ollama not running on :11434 — start with `ollama serve`"

### 6.4 Performance
- Markdown parser runs on every render during streaming — cache rendered lines and only re-render when content changes (compare content hash)
- Avoid allocating new Vec<Line> every frame

---

## Priority 7: Nice-to-Haves

- [ ] `--model <name>` CLI flag to override model on launch
- [ ] `--provider <name>` CLI flag to pick provider
- [ ] `--list-models` flag to list configured models
- [ ] Image support for vision models (pass image paths to Ollama/mimo vision)
- [ ] `/compact` command to manually compress context
- [ ] `/diff` or `/git` to show current git diff as context
- [ ] Syntax highlighting in code blocks via a lightweight approach (regex-based, no syntect)
- [ ] Subagent spawning (like Cursor's "agent in agent")
- [ ] Rules files: read `.cursor/rules/` or `.cairules` for project-specific instructions
- [ ] MCP (Model Context Protocol) support — connect to MCP servers for tools

---

## Current verification routing

The commands below replace the historical install/live-provider recipe. Use
`TESTING.md` for task-specific checks; never copy/install a release binary or
contact a provider as an ordinary validation step.

```bash
cargo fmt --all -- --check
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --release --all-features
```

Release-significant terminal work also needs the native PTY lane documented in
`TESTING.md`; snapshots alone do not prove terminal cleanup.

## Historical non-goals (superseded; see `AGENTS.md` for current rules)
- Don't add syntect for syntax highlighting — too heavy to compile, slows build
- Don't add database dependencies (SQLite, etc.) for sessions — use plain JSON files
- Don't try to connect to Cursor's servers or proxy — the whole point is zero dependency on their infrastructure
- Don't add TUI mouse support beyond basic scroll — cursor-agent doesn't use it much
- Don't implement onboarding/login flows — no auth needed for local APIs
