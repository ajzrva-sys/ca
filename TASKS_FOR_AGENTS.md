# ca (Cursor Agent TUI Rust Clone) — Task List For Low-Skill Agents

> Historical implementation queue. Many tasks are already present in source and some paths/assumptions are stale. Do not execute this file top-to-bottom. Use `NEXT_STEPS.md` for current work and `CONTINUITY.md` for verified state.
# READ THIS ENTIRE FILE BEFORE TOUCHING ANY CODE. DO NOT SKIP SECTIONS.
# -----------------------------------------------------------------------------

## ============== GROUND RULES (BREAK THESE = FAIL) ==============
1. Project root (run all commands from here unless told otherwise):
   /Users/andrewzimmerman/project/ca
2. Build command (run AFTER EVERY SINGLE CHANGE, before moving to next task):
   cd /Users/andrewzimmerman/project/ca && cargo build --release 2>&1
   REQUIREMENT: Output must say "Finished" with ZERO errors AND ZERO warnings. If you see warnings, fix them first.
3. Install command after successful build:
   cp target/release/ca ~/.local/bin/ca
4. HARD PROHIBITIONS (DO NOT DO THESE EVER):
   - Do NOT add any new crate/dependency to Cargo.toml unless a task explicitly tells you to add a specific one
   - Do NOT add syntect (syntax highlighting library), SQLite, or any database — sessions use plain JSON files
   - Do NOT add code that connects to Cursor's servers/proxies — all LLM calls go to local/configured OpenAI-compatible endpoints only
   - Do NOT run rm -rf on anything
   - Do NOT remove/break existing working features: streaming, Ctrl+C cancel, model picker, slash commands, markdown rendering, approval prompts
   - Do NOT change existing code style: 4-space indents, snake_case variable/function names, match existing patterns in the file you are editing
   - Do NOT add unnecessary comments
5. If you get stuck for more than 2 attempts on a task, STOP and report exactly what error you got, do not keep guessing.

## ============== DO THESE TASKS IN ORDER. TOP TO BOTTOM. ==============
# Finish BUG FIXES first, then FEATURES, then POLISH. Do not skip ahead.

---
## BUG FIXES (CRITICAL BROKEN STUFF — DO THESE FIRST)
---

### BUG 1: Headless mode crashes with "model auto not found" 404 error
- **Symptom**: Running `echo "hi" | ca` returns error "model 'auto' not found"
- **Root cause**: The TUI path resolves the special "auto" model name to the first model in the selected provider's model list, but the headless (non-TUI piped input) path does not, it passes literal "auto" to the API.
- **Files to change**: src/main.rs
- **Exact change**:
  1. In the `run_headless()` function (lines 22-29), after loading the provider config, add the same model resolution logic that exists in `run_tui()` at lines 121-129:
     - If provider config exists AND config.defaults.model == "auto", use the FIRST model in provider.models list
     - Otherwise use config.defaults.model as-is
     - If no provider config exists for the default provider, use fallback provider (ollama at http://127.0.0.1:11434/v1, model qwen2.5:0.5b)
- **Verification steps**:
  1. Run build + install commands, confirm no errors/warnings
  2. If Ollama is running: run `echo "say hello in one word" | timeout 15 ~/.local/bin/ca` — it should return a text response, NOT a 404 model error
  3. If Ollama is not running: it should return a connection refused error, NOT a 404 model error — that is acceptable.

### BUG 2: Approval mode from config file is never loaded
- **Symptom**: Changing approval_mode in ~/.config/ca/config.toml to "yolo" still prompts for approval on every tool call
- **Root cause**: Config loads the approval_mode string but never maps it to the ApprovalMode enum in AppState
- **Files to change**: src/main.rs
- **Exact change**:
  1. In `run_tui()`, right after setting `state.current_model` (around line 129), add code to set state.approval based on config.agent.approval_mode:
     - If value is "yolo" / "run-everything" / "everything": set to ApprovalMode::RunEverything
     - If value is "auto-review" / "autoreview" / "auto": set to ApprovalMode::AutoReview
     - All other values (including "allowlist"/default): set to ApprovalMode::Allowlist
- **Verification steps**:
  1. Edit ~/.config/ca/config.toml, change approval_mode under [agent] to "yolo"
  2. Build + install, launch ca in TUI
  3. Send a message that triggers a write_file or run_command tool — it should execute immediately WITHOUT showing a [y/n/a/d] approval prompt
  4. Change config back to "allowlist", relaunch — approval prompt should come back.

### BUG 3: search_files tool crawls build/hidden directories
- **Symptom**: search_files searches in target/, node_modules/, .git/, and other large irrelevant directories, returns garbage results and is slow
- **Root cause**: The walk_dir() function only skips hidden files, not hidden/build directories
- **Files to change**: src/agent/tools.rs
- **Exact change**:
  1. In the `walk_dir()` function (lines 234-258), when processing a directory entry, skip the directory entirely if its name is ANY of these:
     - starts with '.' (hidden directories)
     - equals "target"
     - equals "node_modules"
     - equals ".git"
     - equals "dist"
     - equals "build"
     - equals "__pycache__"
  2. Apply the same skip logic for files: skip files whose name starts with '.', in addition to existing .exe/.bin skip
- **Verification steps**:
  1. Build + install
  2. Run search_files for "fn main" in a Rust project directory that has a target/ folder — it should NOT return any matches from files under target/debug or target/release
  3. It should still find matches in normal .rs source files.

### BUG 4: edit_file replaces only first match with no warning
- **Symptom**: If old_string appears multiple times in a file, edit_file silently replaces only the first occurrence, leading to unintended partial edits
- **Root cause**: uses replacen with count=1, no check for multiple matches
- **Files to change**: src/agent/tools.rs
- **Exact change**:
  1. In the "edit_file" arm (lines 197-216), after reading file content into the `content` variable:
     - Count how many times `old_string` appears in `content`
     - If count == 0: return existing "not found" error
     - If count > 1: return error string: format!("Error: search string found {} times in {}. Use a more unique string to edit only the intended location.", count, resolved)
     - If count == 1: proceed with replacement as before
- **Verification steps**:
  1. Create a test file with two identical lines (e.g. two lines that say "test")
  2. Call edit_file with old_string="test", new_string="changed"
  3. It should return the multiple-match error instead of replacing one line silently
  4. Use an old_string that appears exactly once — it should edit successfully.

### BUG 5: Tool execution uses block_in_place/block_on hack (deadlock risk)
- **Symptom**: Code works but uses a bad async pattern that can deadlock under load
- **Root cause**: Tool execution runs synchronously in the main select! loop using block_in_place + block_on to call async execute_tool
- **Files to change**: src/main.rs
- **Exact change**:
  1. Add a new variant to the StreamMsg enum (line 477): `ToolResults(Vec<(String, String)>)` — tuple of (tool_id, output_string)
  2. Move all tool execution logic out of the synchronous StreamMsg::Finish handler:
     - When Finish is received and tool calls exist, spawn a new tokio task (exactly like start_llm_turn spawns the LLM request task)
     - In this spawned task: run all approval checks, execute tools, collect outputs, send them back via StreamMsg::ToolResults
     - Do NOT use block_in_place or block_on anywhere — use normal .await for execute_tool
  3. Add a handler for StreamMsg::ToolResults in the select! loop:
     - Apply the tool outputs to the matching ToolCall entries in the last assistant message
     - If there are pending approval prompts left, stop and wait for user input (y/n/a/d)
     - If all tools are executed/denied, automatically call start_llm_turn() to continue the agent loop (send tool results to LLM for next turn)
  4. Make execute_pending_tools and execute_single_tool into async functions
- **Verification steps**:
  1. Build + install, no warnings/errors
  2. Ask ca to read src/main.rs and tell you how many lines it has — it should call read_file, return result, answer correctly, no deadlock/hang
  3. Ask it to write a test file, then read it back — write should prompt for approval (in allowlist mode), execute after y, read should return the content you wrote.

### BUG 6: Hardcoded 30-second command timeout
- **Symptom**: Long-running commands always time out after 30 seconds with no way to configure
- **Files to change**: src/config.rs, src/agent/tools.rs, src/main.rs
- **Exact change**:
  1. Add `command_timeout_secs: u64` field to AgentConfig struct in src/config.rs, with default value 30 (set in default_max_turns / default functions section)
  2. Add command_timeout_secs to the Default impl for AgentConfig
  3. Pass the configured timeout value from state/config through to execute_tool when calling it
  4. In the run_command arm of execute_tool, use Duration::from_secs(configured_timeout) instead of hardcoded Duration::from_secs(30)
- **Verification steps**:
  1. Add command_timeout_secs = 60 under [agent] in config.toml
  2. Build + install
  3. Run command `sleep 45 && echo done` — it should complete successfully instead of timing out at 30s.

### BUG 7: Needs_tools() keyword check blocks tools for valid requests
- **Symptom**: Sometimes the LLM can't call tools even when it needs to, because the keyword check didn't match the user's phrasing
- **Fix (simple, no downsides)**: Remove the needs_tools() function entirely. Always send the full tool list to the LLM on every request. The LLM is smart enough to know when to use tools, the keyword check is unnecessary and causes failures.
- **Files to change**: src/main.rs
- **Exact change**:
  1. Delete the entire needs_tools() function (lines 322-334)
  2. In start_llm_turn() (around line 352), replace the if/else that sets tools with just:
     `let tools = agent::tools::default_tool_defs();`
- **Verification steps**:
  1. Build + install
  2. Ask "what files are in this directory" — it should call list_dir and return results correctly
  3. Ask "what is 2+2" — it should answer directly without calling tools (LLM will do this on its own)

---
## FEATURES (DO THESE AFTER ALL BUGS ARE FIXED)
---

### FEATURE 1: Add apply_patch tool
- **Purpose**: Let LLM apply unified diff patches to files, more powerful than edit_file for multi-line changes
- **Files to change**: src/agent/tools.rs
- **Exact change**:
  1. Add a new tool entry to default_tool_defs() named "apply_patch" with parameters:
     - path (string, required): path to file to patch
     - patch (string, required): unified diff format patch to apply
  2. Add an apply_patch arm to execute_tool() that:
     - Resolves path same as other tools
     - Reads existing file content
     - Applies the patch (you can implement simple patch parsing for unified diffs, or use the `diff` crate if needed — but prefer simple logic: split patch into hunks, apply line additions/removals)
     - If patch applies cleanly, write new content and return success
     - If patch fails (lines don't match), return error with context
     - Output truncated to 4096 chars same as other tools
- **Verification steps**:
  1. Create a test file with 3 lines: line1, line2, line3
  2. Call apply_patch with a patch that changes line2 to "line2 changed"
  3. Confirm file content updates correctly
  4. Test with a malformed patch — returns clear error.

### FEATURE 2: @file mention support in input
- **Purpose**: User can type @filename in their message to automatically inject file content into the prompt context
- **Files to change**: src/main.rs (message sending logic), src/app.rs if needed
- **Exact change**:
  1. Before sending a user message (in the should_send block, line 203, after taking input), scan input for @ mentions:
     - Regex pattern: @(\S+) — matches @ followed by non-whitespace characters as a path
     - Resolve each path relative to state.cwd, same as tool path resolution
     - If path exists and is a file, read its content (truncate to 10,000 chars per file)
     - Append a section to the user message: "\n\n--- Attached file: {path} ---\n{content}\n--- End file ---"
     - If path is a directory, append a directory listing (from list_dir logic) instead of file content
     - If path does not exist, leave the @mention as-is and add a note "(file not found: path)" in the appended section
  2. Support up to 10 @mentions per message
- **Verification steps**:
  1. Launch ca, type "explain this code @src/main.rs"
  2. Send message
  3. The LLM should receive the full content of src/main.rs in the user message, and should reference content from the file in its response.

### FEATURE 3: Blinking streaming cursor
- **Purpose**: The ▌ cursor at the end of streaming text should blink on/off every 500ms, like a real terminal cursor
- **Files to change**: src/tui/event.rs, src/tui/render.rs, src/app.rs
- **Exact change**:
  1. Event::Tick already fires every ~50ms — add a blink state boolean to AppState (stream_cursor_visible: bool) and a tick counter
  2. On every Tick event, increment tick counter; every 10 ticks (500ms), toggle stream_cursor_visible between true/false
  3. In render.rs, when rendering in-progress streaming text at the end of a message:
     - If state.stream_cursor_visible is true, append "▌" to the text
     - If false, append " " (space) instead
- **Verification steps**:
  1. Launch ca, send a message that takes a few seconds to stream
  2. Confirm the block cursor at the end of the streaming text blinks on/off regularly.

### FEATURE 4: Persist selected model to config
- **Purpose**: When user selects a model via Ctrl+K picker, save that choice to config.toml so it persists across restarts
- **Files to change**: src/tui/input.rs (model picker selection logic), src/config.rs
- **Exact change**:
  1. When user confirms a model selection in the model picker (presses Enter on a model entry):
     - Update config.defaults.provider to the selected model's provider name
     - Update config.defaults.model to the selected model's id
     - Call config.save() to write updated config back to ~/.config/ca/config.toml
- **Verification steps**:
  1. Launch ca, open Ctrl+K picker, select a different model (e.g. deepseek-chat if key is set, or mlx model)
  2. Quit ca, relaunch it
  3. The title bar should show the model you selected, not the old default ollama model
  4. Confirm ~/.config/ca/config.toml has the new provider/model under [defaults].

### FEATURE 5: Wire up /resume session picker
- **Purpose**: Session save already works, session list/load functions exist in src/session.rs, but the /resume slash command doesn't open a picker UI to restore old sessions
- **Files to change**: src/tui/input.rs (slash command handling), src/tui/render.rs, src/main.rs, src/app.rs (session picker state already exists in AppState)
- **Exact change**:
  1. AppState already has fields for session_picker_open, session_list, selected_session_idx, session_filter — they are unused
  2. When user runs /resume slash command:
     - Call session::list_sessions() to populate state.session_list
     - Set state.session_picker_open = true
     - Set state.session_filter = empty string
     - Set state.selected_session_idx = 0
  3. Add render logic for session picker overlay (model it exactly after the existing model picker render code in render.rs — same size/border/filter input/selected item style, just show session display names instead of models)
  4. Add input handling for session picker (model exactly after model picker handling):
     - Type to filter sessions by name
     - Up/down arrows to move selection
     - Enter to select: call session::load_session() with the selected session ID, replace state.messages with the loaded messages, close picker
     - Esc to close picker without resuming
- **Verification steps**:
  1. Launch ca, send a couple messages so sessions are saved
  2. Quit and relaunch, type /resume
  3. Session picker should appear showing your saved sessions
  4. Select one with Enter, press Enter — old conversation should load into the message list
  5. You can continue chatting in that old session.

---
## POLISH (DO THESE AFTER ALL FEATURES ARE DONE)
---

### POLISH 1: Friendly error messages for common failures
- Files to change: src/agent/client.rs, src/main.rs
- Exact changes:
  1. When API returns connection refused error (can't reach base_url):
     - If base_url is localhost:11434: show error "Could not connect to Ollama at http://127.0.0.1:11434 — start it with `ollama serve`"
     - For other URLs: show "Could not connect to LLM provider at {base_url} — check that it is running and your API key is correct"
  2. When API returns 401 Unauthorized: show error "Invalid API key for {provider} — update your key in ~/.config/ca/config.toml"
  3. When API returns 429 Rate Limited: show "Rate limited by provider — wait a moment and try again"
  4. Replace all .unwrap() calls in client.rs and main.rs with proper ? error handling that returns user-friendly messages instead of panicking.

### POLISH 2: Markdown render caching
- Purpose: Markdown is re-parsed and re-rendered to ratatui Lines on every single frame during streaming, which wastes CPU
- Files to change: src/markdown.rs, src/app.rs, src/tui/render.rs
- Exact changes:
  1. Add a cache to AppState: a HashMap that maps (message_content_hash, content_length) to Vec<Line> (rendered markdown lines)
  2. When rendering a message, first check if content hash exists in cache — if yes, use cached lines
  3. If not, render markdown via pulldown-cmark as before, store result in cache
  4. Clear cache entry for the in-progress streaming message every time new text is appended (so it re-renders with new content)
- Verification: Performance during streaming is smoother, no dropped frames.

### POLISH 3: Ctrl+D / double-Esc quit shortcuts
- Files to change: src/tui/input.rs
- Exact changes:
  1. When input is empty and user presses Ctrl+D: set state.should_quit = true
  2. Add esc_pressed_recently boolean to AppState:
     - When Esc is pressed and no overlays are open: set to true if it was false, if it was already true (pressed twice within 1 second), set should_quit = true
     - Reset esc_pressed_recently to false after 1 second (use Tick events to count time)
  3. When input is not empty: Ctrl+D does nothing, Esc clears input.

### POLISH 4: Simple regex-based code block syntax highlighting
- Purpose: Very light syntax coloring for common languages, no heavy dependencies
- Files to change: src/markdown.rs
- Exact changes:
  1. When rendering code blocks, check the language identifier (rust, python, js, bash, go)
  2. Apply basic color styling:
     - Comments (lines starting with // or # or /* */): dim gray
     - Strings (text inside "" or ''): yellow
     - Common keywords (fn, let, mut, pub, async, await, struct, impl, if, else, for, while, return in Rust; def, import, from, if, else, for, while, return in Python, etc.): cyan
     - Numbers: magenta
  3. Use simple regex matching per line, don't try to build a full parser
- Verification: Code blocks in assistant responses show basic colors for keywords/strings/comments.

---
## FINAL VERIFICATION CHECKLIST (RUN AFTER ALL TASKS)
1. Full release build has zero errors, zero warnings
2. TUI launches without crashing
3. Streaming works, Ctrl+C cancels stream
4. Model picker opens, filters, selects, persists choice
5. Slash commands work (/help, /clear, /resume, /quit)
6. All 7 tools work: read_file, write_file, list_dir, run_command, search_files, edit_file, apply_patch
7. Approval modes work (allowlist prompts, yolo auto-approves)
8. Multi-turn agent loop works: LLM calls tool → executes → returns result → LLM continues answering
9. @ mentions inject file content
10. Sessions save, /resume loads old sessions
11. Headless piped mode works without model errors
12. Config settings (approval mode, command timeout, default model) load correctly
