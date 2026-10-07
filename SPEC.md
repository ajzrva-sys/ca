# ca — Complete Feature Spec (1:1 Cursor Agent TUI clone)

> Historical parity inventory, not current implementation truth. Several status sections and paths are stale. Use `docs/.INDEX.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, and `PARITY.md`; verify every behavior against primary source/tests and read-only oracle evidence before acting.

## Architecture Overview

`ca` replicates the Cursor Agent CLI TUI in Rust. Source: `~/project/ca/`. Binary: `~/.local/bin/ca`.
Config: `~/.config/ca/config.toml`. Sessions: `~/.local/share/ca/sessions/`.

**Stack:** ratatui 0.29, crossterm 0.28, tokio, reqwest, serde, pulldown-cmark 0.12.

The entire TUI of cursor-agent is ~458 ink/React components in `1683.index.js` (1MB). All rendering is local.
Only the LLM agent worker goes to the network (we replace `agentn.global.api5.cursor.sh` with local OpenAI-compatible APIs).

---

## 1. Constants & Symbols (from `constants.ts`)

```
promptSymbol   = "→ "      (input prompt prefix, cyan bold)
indent         = "  "      (2-space indent for continuation)
bullet         = " • "     (list items)
radioFilled    = "● "      (selected radio)
radioEmpty     = "○ "      (unselected radio)
checkboxOn     = "◉ "      (checked)
checkboxOff    = "◯ "      (unchecked)
checkmark      = "✓"       (success/completed tools)
hPad           = 1         (horizontal padding)
vPad           = 2         (vertical padding)
streamCursor   = "▌"       (blinking cursor at end of streaming text)
```

Colors used by cursor-agent (chalk/ink Ay.* calls):
- `white` — primary text
- `cyan` — prompts, links, emphasis, selected items, title bar bg
- `yellow` — warnings, pending/running tools, inline code text
- `green` — success, completed tools, checkmarks
- `red` — errors, rejected operations, deletions
- `blue` — URLs, user message prefix, diff additions?
- `magenta` — special markers
- `gray` / `dim` — secondary text, placeholders, hints
- `DarkGray` bg — inline code background

Title bar: black text on cyan bg (inverted), bold. Right side hints: black on dark gray bg.

---

## 2. Layout Structure

```
┌─────────────────────────────────────────────────────────┐
│ ca · agent · qwen2.5:0.5b (ollama)     Ctrl+K · / · ⌃C│  1 row, title bar
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ▸ Hello, how can I help?                               │
│                                                         │  Min 3 rows, conversation
│  Hello! I can help you with...                          │  (fills remaining space)
│                                                         │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ → ▌Ask anything...                                       │  3 rows (border top + input)
├─────────────────────────────────────────────────────────┤
│ 2 messages · Enter send · Shift+Enter newline            │  1 row, status bar
└─────────────────────────────────────────────────────────┘
```

Overlays (centered, bordered boxes):
- Model picker (Ctrl+K): 70x20, cyan border, filter input, list with ▸ selection
- Slash command palette (/): 50x16, cyan border
- Approval prompts: inline in conversation area (not modal)
- Tool argument editors: modal boxes

---

## 3. Keybinds (complete list)

### Global
| Key | Action |
|-----|--------|
| `Ctrl+C` | When idle: press twice to quit. When streaming: cancel generation. |
| `Ctrl+D` | On empty input: quit. Otherwise: EOF behavior. |
| `Ctrl+L` | Clear screen and conversation (when agent not working) |
| `Ctrl+K` | Open model picker (fuzzy filter) |
| `/` | Open slash command palette (when input empty) |
| `Tab` | Cycle mode: agent → plan → ask → debug → agent |
| `Shift+Tab` | Reverse cycle |
| `Esc` | Close overlays/palettes. Dismiss status messages. |
| `↑`/`↓` | Navigate input history (when not in overlay) |
| `PageUp`/`PageDown` | Scroll conversation up/down |
| `Home`/`End` | Scroll to top/bottom (also input cursor movement) |

### Input Box
| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | Insert newline |
| `Ctrl+J` | Insert newline (alternative to Shift+Enter) |
| `Alt+Enter` | Insert newline |
| `←`/`→` | Move cursor |
| `Ctrl+A` / `Home` | Cursor to start |
| `Ctrl+E` / `End` | Cursor to end |
| `Ctrl+W` | Delete word backwards |
| `Ctrl+U` | Delete entire line |
| `Backspace` | Delete character before cursor |
| `Ctrl+B` | Move back one word (Alt+B also works) |
| `Ctrl+F` | Move forward one word (Alt+F also works) |
| `Ctrl+K` (input non-empty) | Kill to end of line |
| `Ctrl+Y` | Yank (paste killed text) |

### During Streaming
| Key | Action |
|-----|--------|
| `Ctrl+C` | Cancel generation |
| `Ctrl+O` | Expand/collapse truncated content (plan, long outputs) |

### Model Picker
| Key | Action |
|-----|--------|
| `↑`/`↓` | Navigate models |
| `Enter` | Select |
| `Esc` | Close without changing |
| Type | Filter models (case-insensitive fuzzy) |
| `Backspace` | Delete filter character |

### Slash Palette
| Key | Action |
|-----|--------|
| `↑`/`↓` | Navigate commands |
| `Enter` | Execute selected command |
| `Esc` | Close |
| Type | Filter |

### Vim Mode (toggle with `/vim`)
When enabled, input supports vi-style keybindings (normal/insert modes).

### Additional cursor-agent binds
| Key | Action |
|-----|--------|
| `Ctrl+R` | Review changed files (show git diff summary) |
| `Ctrl+S` | Save conversation/plan |
| `Ctrl+G` | Alternative escape/cancel |
| `Ctrl+O` | Expand/collapse content blocks |
| `q` | Close pager/overlay views |
| `j`/`k` | Scroll in pagers (vim-style) |
| `g`/`G` | Top/bottom in pagers |
| `Space`/`f` | Page down in pagers |
| `b` | Page up in pagers |

---

## 4. Slash Commands (complete list from source)

Core commands (must implement):
| Command | Short | Description | Auto-execute |
|---------|-------|-------------|-------------|
| `/model` | `/m` | Switch model | yes |
| `/plan` | `/p` | Enter plan mode | yes |
| `/ask` | `/a` | Enter ask mode | yes |
| `/debug` | `/d` | Enter debug mode | yes |
| `/run-everything` | `/y`, `/re` | Auto-approve all tools (YOLO) | yes |
| `/auto-review` | `/ar` | Toggle auto-review mode | yes |
| `/clear` | `/c` | Clear conversation | yes |
| `/quit` | `/q` | Quit | yes |
| `/help` | `/h` | Show help | yes |

Important commands to implement:
| Command | Description |
|---------|-------------|
| `/vim` | Toggle Vim mode in text inputs |
| `/approval-mode` | Switch between allowlist/auto-review/yolo |
| `/config` | Open config in $EDITOR |
| `/resume` | Resume a past session |
| `/fork` | Fork conversation from a message |
| `/rewind` | Rewind to before a previous message |
| `/context` | Show/manage context |
| `/add-dir` | Add directory to context |
| `/shell` | Enter shell mode (prefix with !) |
| `/mcp` | MCP server management (list/list-tools) |
| `/sandbox` | Configure sandbox settings |
| `/commit` | Commit changes |
| `/changes` | Show git diff of all changes since session start |
| `/line-numbers` | Toggle line numbers in code blocks |
| `/show-thinking` | Toggle thinking block visibility |
| `/hints` | Toggle hints display |
| `/name` | Name the session |
| `/summarize` | Summarize conversation |
| `/copy` | Copy last message/code to clipboard |
| `/full-conversation` | Toggle full conversation rendering |
| `/load-workspace` | Load workspace state |
| `/save-workspace` | Save workspace state |
| `/skills` | List/manage skills |
| `/rule` | Add/edit rules |
| `/jobs` | Show background jobs |
| `/notifications` | Toggle notifications |
| `/version` | Show version |

Nice-to-have (can skip for v1):
`/about`, `/action`, `/attribute-prs-to-agent`, `/bedrock`, `/btw`, `/channel`, `/command`, `/copy-conversation-id`, `/copy-request-id`, `/count`, `/create-new`, `/cursor`, `/debug-test`, `/fast`, `/feedback`, `/filter`, `/hash`, `/id`, `/identifier`, `/line-numbers`, `/logs`, `/max-mode`, `/message`, `/mode-switch-shift-tab`, `/model-slash-commands`, `/no-sandbox`, `/open`, `/options`, `/path`, `/permissions-allow`, `/permissions-deny`, `/plugin`, `/pq`, `/prompt`, `/question`, `/rename`, `/setup-terminal`, `/show-line-numbers`, `/show-status-indicators`, `/show-thinking-blocks`, `/state`, `/static-indicator`, `/status-indicators`, `/subcommand`, `/sync-theme`, `/throw`, `/timeout`, `/uncategorized`, `/update`, `/url`, `/usage`

---

## 5. Tool System (from component analysis)

Cursor has these built-in tools (visible as separate React components):

| Tool | Component file | Description |
|------|---------------|-------------|
| **read** | `read-tool-ui.tsx` | Read file contents |
| **write/edit** | `edit-tool-ui.tsx` + `edit-tool-diff.ts` | Edit file with diff preview |
| **delete** | `delete-tool-ui.tsx` | Delete file |
| **ls** | `ls-tool-ui.tsx` | List directory |
| **glob** | `glob-tool-ui.tsx` | Find files by glob pattern |
| **grep** | `grep-tool-ui.tsx` | Search file contents |
| **shell** | `shell-tool-ui.tsx` | Execute shell command |
| **sem-search** | `sem-search-tool-ui.tsx` | Semantic code search |
| **web-search** | `web-search-tool-ui.tsx` | Search the web |
| **web-fetch** | `web-fetch-tool-ui.tsx` | Fetch URL content |
| **create-plan** | `create-plan-tool-ui.tsx` | Create/edit a plan |
| **task** | `task-tool-ui.tsx` | Task/subagent management |
| **await** | `await-tool-ui.tsx` | Wait for background tasks |
| **read-lints** | `read-lints-tool-ui.tsx` | Read lint errors |
| **read-todos** | `read-todos-tool-ui.tsx` | Read TODO comments |
| **update-todos** | `update-todos-tool-ui.tsx` | Update TODO tracking |
| **generate-image** | `generate-image-tool-ui.tsx` | Generate images |
| **get-mcp-tools** | `get-mcp-tools-tool-ui.tsx` | List MCP tools |
| **fetch-mcp-resource** | `fetch-mcp-resource-tool-ui.tsx` | Fetch MCP resource |
| **list-mcp-resources** | `list-mcp-resources-tool-ui.tsx` | List MCP resources |
| **mcp** | `mcp-tool-ui.tsx` | Call MCP tool |
| **write-shell-stdin** | `write-shell-stdin-tool-ui.tsx` | Write to shell stdin |
| **switch-mode** | `switch-mode-tool-ui.tsx` | Switch agent mode mid-conversation |
| **ask-question** | `ask-question-tool-ui.tsx` | Ask user a clarifying question |

### Tool flow
1. LLM emits `tool_calls` in streaming response
2. For each tool, check approval mode:
   - **YOLO mode** (`/y`): auto-execute everything
   - **Auto-review**: auto-execute "safe" tools (read, ls, glob, grep, search), prompt for destructive ones
   - **Allowlist** (default): prompt for every tool execution
3. Approval prompt shows: tool name, args preview, options:
   - `y` / Enter — run once
   - `n` / Esc — deny
   - `a` — always allow this tool (add to allowlist)
   - `d` — deny and add to denylist
4. After execution, output shown inline (truncated at ~2000 chars / 1MiB for shell)
5. Tool result fed back to LLM as `role: tool` message
6. LLM continues (multi-turn, up to max_turns=100)

### Approval UI (inline, not modal)
```
  ⟳ Run this command?
    cat /etc/hosts
  [y] run  [n] deny  [a] always allow  [d] deny always
```

---

## 6. Conversation Rendering

### User messages
- Prefix: `> ` (cyan, bold)
- Continuation lines: `  ` (2 spaces, cyan)
- Color: cyan

### Assistant messages
- Regular text: white
- Bold/italic: rendered via markdown
- Code blocks: bordered with box-drawing characters
- Inline code: yellow text on dark gray background
- Tool calls: indented 2 spaces, with status icon

### Tool call display format
```
  ✓ Read file              src/main.rs
    (first 10 lines of file content shown in dim gray)
```
Running: `⟳` yellow, Completed: `✓` green, Error/Rejected: `✗` red, Pending: `⋯` gray

### Code blocks
- Border: `┌─┐ │ └─┘` box drawing
- Language label shown if specified (e.g., `│ rust`)
- Line numbers optional (toggle with `/line-numbers`)
- Truncation: `…(plan truncated) ctrl+o to expand` for long content
- Max display height: 20 lines, expand with Ctrl+O

### Diff display (edit tool)
- Added lines: green `+` prefix
- Removed lines: red `-` prefix  
- Context lines: white, ` ` prefix
- File path header in cyan

### Streaming
- Text appears character-by-character
- Blinking block cursor `▌` at end of current text
- Long responses auto-scroll to bottom
- `⟳ Thinking…` shown before first token arrives

### Truncation
- Shell output truncated at 1MiB: "Output truncated at 1MiB limit"
- Display truncated at configurable line count
- `Ctrl+O` toggles expand/collapse

---

## 7. Input Box Details

- Prompt symbol: `→ ` (cyan, bold)
- Placeholder: "Ask anything… (Shift+Enter for newline)" when empty
- When streaming: "Ctrl+C to stop generation…"
- Grows up to 8 lines tall for multiline input
- When multiline, conversation area shrinks accordingly
- Block cursor: white background, black foreground character
- Supports bracketed paste for multi-line paste
- Ghost text / autocomplete suggestions shown dimmed after cursor

---

## 8. Status Bar

Bottom 1 row. Shows:
- Idle: `N messages · Enter send · Shift+Enter newline · ↑↓ history · / commands`
- Streaming: `generating…` or `⟳ Running: <tool name>` with elapsed time
- After completion: `Done (stop)` or token usage if available
- Error: yellow error message
- Status messages fade after 3 seconds or on next keypress

---

## 9. Title Bar

Top 1 row:
- Left: `ca · <mode> · <model> (<provider>)` with cyan bg, black text, bold
- Middle: fills with dark gray bg
- Right: `Ctrl+K models · / commands · Ctrl+C quit` with dark gray bg, black text

Modes: `agent`, `plan`, `ask`, `debug`
Approval indicator: ` · yolo` or ` · auto-review` appended when non-default

---

## 10. Welcome Screen (empty conversation)

Centered content:
```
  Cursor Agent (local)

  Ask anything. Use tools. Iterate on code.

  Shortcuts:
    • Enter        send message
    • Shift+Enter  new line
    • Ctrl+J       new line
    • Ctrl+K       switch model
    • Ctrl+L       clear chat
    • Ctrl+C       cancel / quit
    • ↑/↓          history
    • /            commands
```

When in a git repo, show repo name/branch in welcome.

---

## 11. Model Picker (Ctrl+K)

```
┌────────────────────────────────────────────────────────────┐
│ Switch model                                               │
│                                                            │
│ > type to filter…                                          │
│                                                            │
│ ▸ qwen2.5:0.5b (ollama)                                    │
│   deepseek-chat (deepseek)                                 │
│   doubao-seed-2.0-pro (ofox)                               │
│   mimo-v2.5 (mimo)                                         │
│   mlx-community/Qwen3-Coder-30B (mlx)                     │
│                                                            │
│ Enter select · Esc close · ↑↓ navigate                     │
└────────────────────────────────────────────────────────────┘
```

- Fuzzy filter as you type (case-insensitive, matches model name OR provider)
- Selected item highlighted: black text on cyan bg
- Groups by provider
- Type model name not in list → uses it as custom model ("manual" provider)

---

## 12. Slash Command Palette (/)

```
┌──────────────────────────────────────────────┐
│ Commands                                     │
│                                              │
│ /model   Switch model                        │
│ /plan    Plan mode                           │
│ /ask     Ask mode                            │
│ /debug   Debug mode                          │
│ /yolo    Auto-approve all                    │
│ /clear   Clear conversation                  │
│ /quit    Quit                                │
│                                              │
│ Enter run · Esc close                        │
└──────────────────────────────────────────────┘
```

Filters as you type. Commands with `autoExecuteOnAccept: true` run immediately on Enter.

---

## 13. Agent Loop

```
User sends message
  ↓
Build messages array (system + history + user message)
  ↓
Stream chat completion from provider
  ↓
Accumulate text deltas → render in real-time
  ↓
If tool_calls received:
  ├─ For each tool call:
  │   ├─ Check approval (allowlist/auto-review/yolo)
  │   ├─ If needed, prompt user (y/n/a/d)
  │   ├─ Execute tool (file op / shell / etc.)
  │   ├─ Show result in conversation
  │   └─ Append tool result as role=tool message
  ├─ Continue streaming (feed tool results back to LLM)
  └─ Repeat until LLM stops calling tools (max 100 turns)
  ↓
Final assistant message saved to conversation
  ↓
Auto-save session to disk
  ↓
Ready for next user input
```

### System prompt content
- "You are a coding assistant running in a CLI."
- "You have access to tools for reading/writing files and running commands."
- Working directory path
- Platform info (macOS, shell)
- Instructions to be concise, use tools proactively

### Multi-turn tool use
- LLM can call multiple tools per turn
- Tools can be called in sequence (read → edit → test)
- Each tool call shows in conversation as it happens
- User can cancel entire agent loop with Ctrl+C

---

## 14. Sandbox/Approval Modes

Three modes (cursor has these as separate concepts):

1. **Allowlist** (default): Prompt for every tool. User can add specific tools/paths to allowlist.
2. **Auto-review**: Safe tools (read, ls, glob, grep, sem-search) auto-approve. Destructive tools (write, delete, shell) prompt.
3. **Run Everything (YOLO)**: All tools auto-approve, no prompts. Indicator in title bar.

Allowlist entries: tool name + path pattern (e.g., "Read ~/project/**", "Shell npm test")

---

## 15. Session Persistence

- Auto-save every message to `~/.local/share/ca/sessions/<timestamp>-<name>.json`
- JSON format: `{ version, model, created, messages: [{role, content, tool_calls?, timestamp}] }`
- `/resume` lists sessions by date, pick to resume
- `/fork <n>` forks from message N (creates new session branched from there)
- `/rewind <n>` rewinds to before message N (undoes everything after)
- `/name <name>` names the current session
- `/summarize` generates a summary of the conversation so far

---

## 16. Configuration

TOML at `~/.config/ca/config.toml`:
```toml
[defaults]
provider = "ollama"
model = "auto"

[agent]
max_turns = 100
approval_mode = "allowlist"  # allowlist | auto-review | yolo

[providers.ollama]
base_url = "http://127.0.0.1:11434/v1"
api_key = "ollama"
models = ["qwen2.5:0.5b"]

[providers.mimo]
base_url = "https://token-plan-sgp.xiaomimimo.com/v1"
api_key = "${MIMO_API_KEY}"
models = ["mimo-v2.5"]

[providers.ofox]
base_url = "https://api.ofox.ai/v1"
api_key = "${OFOX_API_KEY}"
models = ["volcengine/doubao-seed-2.0-pro"]

[providers.deepseek]
base_url = "https://api.deepseek.com"
api_key = "${DEEPSEEK_API_KEY}"
models = ["deepseek-chat"]

[providers.mlx]
base_url = "http://127.0.0.1:8080/v1"
api_key = "mlx"
models = ["mlx-community/Qwen3-Coder-30B-A3B-Instruct-4bit"]
```

Environment variable expansion: `${VAR_NAME}` in config strings.

Settings (toggleable via slash commands, persisted to config):
- `vim_mode` (bool, default false)
- `show_line_numbers` (bool, default false)
- `show_thinking_blocks` (bool, default true)
- `show_hints` (bool, default true)
- `model_slash_commands` (bool, default true)
- `show_status_line_running_time` (bool, default true)

---

## 17. Shell Mode

Enter with `/shell` or type `!` on empty input:
- Commands executed directly in shell instead of going through agent
- `!ls -la` runs ls directly
- `!` alone enters interactive shell mode
- Esc exits shell mode back to agent

---

## 18. Plan Mode

- `/plan` enters plan mode
- Agent proposes a plan first, asks for approval before executing
- Plan displayed with checkboxes for each step
- `Enter` accepts, `Esc` rejects, can edit plan
- Plan can be truncated: `…(plan truncated) ctrl+o to expand`

---

## 19. Pager / Scroll View

For long outputs (large code blocks, logs, diffs):
- `j`/`k` line up/down
- `Ctrl+D`/`Ctrl+U` half page
- `Ctrl+E`/`Ctrl+Y` one line
- `g`/`G` top/bottom
- `Space`/`f` page down
- `b` page up
- `q`/`Esc` close

---

## 20. Vim Mode

Toggle with `/vim`. When on:
- Input starts in normal mode
- `i`/`a` enter insert mode
- `Esc` returns to normal mode
- `h`/`j`/`k`/`l` movement in normal mode
- `w`/`b` word movement
- `0`/`$` start/end of line
- `d`/`c`/`y` delete/change/yank operations
- `u` undo, `Ctrl+R` redo

---

## 21. Markdown Rendering Details

pulldown-cmark parsing, output to ratatui Line/Span:

| Element | Rendering |
|---------|-----------|
| `# H1` | `## ` prefix, cyan bold |
| `## H2` | `## ` prefix, cyan bold |
| `### H3` | `### ` prefix, cyan bold |
| `**bold**` | bold modifier |
| `*italic*` | italic modifier |
| `` `code` `` | yellow text, dark gray background |
| ` ```lang ` | Box-drawn code block with language label |
| `- item` | `  • ` yellow bullet |
| `1. item` | `  N. ` numbered |
| `[text](url)` | show URL in blue underlined |
| `---` | horizontal rule (`─` × 40, dark gray) |
| `> quote` | `│ ` prefix, gray |

---

## 22. Current Code State

Working (verified):
- TUI launches and renders
- Title bar, conversation, input, status bar
- Markdown rendering (headings, bold, italic, code, lists, links, rules)
- Code blocks with box borders
- Non-blocking streaming via tokio select!
- Ctrl+C cancel mid-stream
- Model picker with fuzzy filter
- Slash command palette
- Input with cursor movement, history, word delete
- Auto-scroll during streaming
- Welcome screen
- Headless mode for piping
- Ollama integration (qwen2.5:0.5b verified)
- Multi-provider config (ollama, mimo, ofox, deepseek, mlx)

Not yet implemented (the work):
- Tool execution (file ops, shell commands)
- Multi-turn agent loop (tool call → execute → feed back → continue)
- Approval prompts (y/n/a/d inline UI)
- Diff display (green/red +/- lines)
- Session persistence (save/resume/fork/rewind)
- Multiline input growth (input box currently fixed at 1 line)
- Blinking stream cursor
- Paste support (bracketed paste)
- Vim mode
- Shell mode (! commands)
- Plan mode with approval
- Pager (j/k/g/G/space for long content)
- Ctrl+O expand/collapse
- Ctrl+R review changed files
- Many slash commands
- Syntax highlighting in code blocks
- Line numbers in code blocks
- Ghost text / autocomplete
- Token count display
- Elapsed time in status bar
- @file mentions in input
- MCP support

---

## 23. File Map

```
~/project/ca/
├── Cargo.toml
├── TODO.md
├── SPEC.md              (this file)
├── reference/           (extracted cursor-agent JS snippets)
├── src/
│   ├── main.rs          — entry, select! loop, dispatch, tool execution flow
│   ├── app.rs           — AppState, Message, ToolCall, ModelInfo, enums
│   ├── config.rs        — TOML config, ${ENV} expansion, defaults
│   ├── markdown.rs      — pulldown-cmark → ratatui Line/Span
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── event.rs     — crossterm setup, async event polling
│   │   ├── input.rs     — keybind handling, picker/palette logic
│   │   └── render.rs    — all ratatui rendering
│   └── agent/
│       ├── mod.rs
│       ├── client.rs    — OpenAI-compat HTTP streaming client
│       └── loop_.rs     — (unused, to be completed or deleted)
```

Target: `~/project/ca/src/agent/tools.rs` (new) for tool definitions and execution.
