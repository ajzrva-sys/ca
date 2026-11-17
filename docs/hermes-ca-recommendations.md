# Hermes recommendations for CA

This file is a comparison against ca 0.1.112. It is not a work queue. By ca 0.1.130 the tree already has busy submit, precise edits, image retirement, the repetition stop, empty-reply repair, the coding-schema omit list, compaction-model titles, the goal footer, OSC 52 copy, transcript windowing, the live-child dock above the composer, and the direct-check follow-up. Do not rebuild those items from this list.

Compared `oracles/hermes-agent` at `2597dc6b7e876e57f47fabeb1c4bc4efc1e12e85`
(2026-11-13T00:01:55+01:00) with CA 0.1.112. The previous oracle commit was
`4eba3eb` (2026-10-11). The checkout is shallow. No Hermes source was copied
into `src/`.

The UI and backend items below are implemented in the CA source. Queue stays
the default while a turn is running. Auto-review, the sandbox, and the provider
filter are unchanged. Similarity edit strategies, a second auxiliary client,
and `/insights` were not added.

Each item names the Hermes file, the CA file, and the change. Items CA already
does are listed at the end so they are not rebuilt.

## UI

### 1. Pin live children above the composer

Hermes shows a dock above the composer while children are running
(`ui-tui/README.md`, `ui-tui/src/components/agentsPanel.tsx`). The dock shows
the live count, task name, elapsed time, and the latest activity. Short
terminals get fewer rows. Finished children leave the dock and stay in history.
Ctrl+T expands the roster without clearing the draft. `e` queues steering for
the next tool boundary. `x` stops the selected child. `X` stops the subtree.

CA already draws that information inside the transcript. `render_agent_activity_group`
in `src/tui/render.rs` prints a "Subagents" group with role, phase, and elapsed
time. `handle_agent_activity_focus` in `src/tui/input.rs` selects a row, opens
detail, sends a message with `m`, and cancels one child with `c` after a
confirmation.

Add a dock that reads `live_agent_activity` and reuses those keys. Keep it
above the composer for as long as any child is live, including when the
transcript is scrolled. Shrink the row budget when the terminal is short.
Finished children stay in the transcript group. Add a subtree stop. Steering
from the dock uses the existing child message path and means "accepted for the
next tool boundary."

### 2. Show the busy-composer mode, and add steer

Hermes `ui-tui/src/app/useSubmission.ts` has three submit modes while a turn
is running:

- `queue` appends the draft and drains it when the turn finishes.
- `steer` injects into the current turn at the next safe boundary, and falls
  back to the queue if steer is rejected.
- `interrupt` redirects the active model request. This is the Hermes default.

CA's default is the queue. While a turn is running, Enter queues the draft.
The selected row has Send now, Edit, and Cancel. Ctrl+S, Send now, and Enter
on an empty composer stop the turn and send (`CONTINUITY.md`). A mid-turn
steer that does happen is wrapped by `steer_model_text` in `src/main.rs`.

Keep queue as the default. Paint the active mode on the composer footer.
Add a key that cycles queue, steer, and interrupt for the next submit.
Steer calls `steer_model_text` at the next tool boundary and leaves the
stream running. Interrupt stays the existing stop-and-send path.

### 3. Draw a window of the transcript

Hermes virtualizes history (`ui-tui/src/hooks/useVirtualHistory.ts`,
`ui-tui/src/lib/virtualHeights.ts`) and renders the streaming assistant
incrementally (`ui-tui/src/components/streamingMarkdown.tsx`).

CA walks every message in `conversation_content` (`src/tui/render.rs`) and
builds each assistant body with `render_safe_markdown_document`. A single user
card already keeps a row window (`render_user_prompt_card`). The transcript
scroll then clips a fully built line list.

Measure and draw the visible rows plus a small overscan. Update the streaming
assistant in place. Leave the user-card window as it is.

### 4. Show the goal condition

Hermes `ui-tui/src/components/goalBar.tsx` paints one row while a goal is
standing: glyph, label, turn count, and the goal title.

CA stores the condition on `GoalState.condition` (`src/goal.rs`). The footer
in `src/tui/render.rs` appends `goal {status} {continuations}/{max}` and drops
the condition.

Put a truncated condition on that same footer detail. One row is enough. A
second standing row is only needed when the footer is already full.

### 5. Color an in-progress slash or @ token

Hermes `ui-tui/src/domain/composerHighlights.ts` colors a `/command`, an
`@file` / `@url` / `@session` reference, and a paste token. Joining the spans
reproduces the input. The styling does not rewrite the text.

CA already colors committed composer atoms. `composer_row_spans` in
`src/tui/render.rs` paints `AtomicKind` ranges from `src/tui/composer.rs`
(reference, file, folder, large paste). A slash token and an `@` token that
are not yet atoms stay plain.

Color those two token shapes with the same accent used for atoms. Enter still
sends the composer string unchanged.

### 6. Fall back to OSC 52 when the local clipboard fails

Hermes copies through OSC 52 (`ui-tui/src/lib/osc52.ts`) so a remote terminal
can receive the selection.

CA builds an OSC 52 sequence in `terminal_clipboard_sequence` (`src/main.rs`)
for the provider sign-in URL. Mouse selection and the copy commands use
`arboard` from `src/tui/input.rs`.

When `arboard` returns an error, write the existing OSC 52 sequence for the
selected transcript text, including the tmux passthrough form already used
for the sign-in URL.

## Backend

### 1. Add `execute_code` that calls CA tools through a parent socket

Hermes `tools/code_execution_tool.py` lets one model turn run a Python script
whose stubs call Hermes tools over a Unix socket (loopback TCP on Windows).
A per-session token authenticates the socket. The model receives the script's
stdout. Remote backends fall back to a file RPC. The parent keeps approval,
the interpreter, and the working directory.

CA compiles one std-only Rust program (`fn main` plus `ca_tool(name, args_json)`) and runs that binary under the `run_command` sandbox. It does not run Python.

Add one tool, `execute_code`, on the Main and Subagent profiles. The program
runs under the same sandbox, path, and network policy as `run_command`. The
parent listens on a session socket protected by a per-session token. Each
stub call enters the existing tool handler and the existing approval
classification. The script cannot add tools, widen paths, or change the
approval mode. Return bounded stdout to the model. Kill the kernel when the
session ends. Time out a single stub call.

### 2. Apply a precise edit chain, and report which strategy matched

Hermes `tools/fuzzy_match.py` tries, in order: `exact`, `line_trimmed`,
`whitespace_normalized`, `indentation_flexible`, `escape_normalized`,
`trimmed_boundary`, `unicode_normalized`, then two similarity strategies
(`block_anchor`, `context_aware`). Similarity matches are refused for
`replace_all`. `is_already_applied` treats a re-sent edit as a no-op when the
new text is already present exactly (at least 8 characters) and the old text
is gone.

CA's string edit is exact. `src/edit_followup.rs` names a smart quote, em
dash, ellipsis, or non-breaking space and does not rewrite the file.

Use the seven precise strategies on the exact-string edit tools. A strategy
that matches more than once fails unless replace-all was requested. Put the
strategy name in the tool result. Add the already-applied no-op. Leave
`block_anchor` and `context_aware` out until a test shows a unique match that
the precise chain cannot make.

### 3. Retire images only when a provider limit would be crossed

Hermes `agent/image_eviction_policy.py` retires old tool-result images on a
fresh copy of the outbound request. It does this when a real provider limit
would be crossed (image count, byte budget, or the Anthropic 20-image
dimension cap when some image exceeds 2000 px). A keep-newest window that
moves every turn is rejected there because it rewrites the cached prefix on
every image.

CA accounts image tokens with the provider tile rule (`CONTINUITY.md`).
`src/context_pruning.rs` replaces old successful read and search results with
a marker. It does not retire images, so screenshots stay in every later
request until a full compaction.

On the request copy, retire a batch of old tool-result images to a text
marker when the next request would cross that provider's image-count or byte
limit. Leave the session history intact. One batch per crossing, so the
cached prefix stays stable between crossings.

### 4. Stop a repetition loop without sending the looped text back

Hermes `agent/repetition_guard.py` checks fragments of at least 400
characters. A 60-character window that repeats enough to cover half the
fragment aborts the turn. The stored partial is the marker
`[the reply degenerated into a repetition loop and was interrupted]`, so the
next request does not re-seed the loop.

CA `src/prose_recovery.rs` retries once when a finished assistant reply
repeats and called no tool. It does not watch a stream that is echoing one
fragment.

Run the same window check on the streaming assistant text. On a hit, stop the
turn and store the marker. Keep `retry_once` for a finished duplicate reply.

### 5. Retry one empty reply, then stop, after repairing a wedged tail

Hermes `agent/turn_empty_response.py` recovers a reply that has no visible
text. `agent/turn_final_response.py` says a malformed tail (`tool` then
`user`, or `user` then `user`) makes most providers return empty content and
retriggers that recovery forever. `repair_message_sequence_with_cursor` in
`agent/turn_iteration_prep.py` runs before the request.
`agent/context_compressor.py` also strips orphan `tool_calls` from a request
copy and refuses to split a tool group at a compaction boundary.

CA already refuses to send a prefix that splits an unresolved tool exchange
(`protocol_complete_prefix_len` in `src/context.rs`). It does not repair a
wedged tail, and it has no empty-reply retry.

On the request copy, merge a `user`/`user` tail and stub an assistant
`tool_call` that has no result, before the provider call. If the assistant
message then has no visible text, retry once with a short reminder. A second
empty result ends the turn with a visible empty outcome. Do not add Hermes's
provider-fallback or thinking-prefill ladder.

### 6. Ask for a direct check after a code edit

Hermes `agent/verification_stop.py` does not run the check itself. When the
model is about to finish after editing code, and the latest command is not a
fresh check, Hermes appends one bounded follow-up. Edits that touch only
prose (`.md`, `.txt`, `LICENSE`, `CHANGELOG`, and the same list in that file)
do not ask. A piped command whose exit status is not the check's own status
does not count.

CA's auto-review path (`src/automatic_review.rs`) is a separate reviewer. It
is the right path when that mode is already on.

When auto-review is off, a turn that edited code and is about to finish
without a fresh direct command result gets one follow-up naming the changed
paths and asking for the check as the first command in the pipeline. Prose-only
edits skip the follow-up. Do not start a second reviewer.

### 7. Drop email and image generation from the default coding schema

Hermes `toolsets.py` uses a coding posture in a code workspace. That posture
omits image generation, text-to-speech, cron, kanban, and computer-use so the
tool schema stays small. The full set remains available.

CA puts `email_accounts`, `email_list`, `email_read`, `email_draft`,
`email_send`, `email_reply`, and `generate_image` on `ToolProfile::Main`
(`src/agent/registry.rs`). Those schemas go out on every coding turn.

In a git workspace, omit those seven tools from the default schema. A command
puts them back for the session. The handlers stay. `ask_chatgpt` stays on the
default schema.

### 8. Refuse tool reads and writes of credential files and `.env`

Hermes `agent/file_safety.py` is a tool error, not a sandbox. `get_read_block_error`
refuses credential stores under the Hermes home, profile homes, and any
`.env` file (`.env.example` remains readable). `is_write_denied` refuses
writes into the agent home's credential paths. The terminal can still reach
those files as the same OS user.

CA's edit and read tools do not have that denial. The provider filter can
still mask outbound text after a read has succeeded.

`read_file`, search, and the edit tools return an error for `~/.config/ca`
credential files and for a `.env` file. `.env.example` stays readable. The
error says the provider client consumes those credentials. This does not
replace the sandbox or the provider filter.

### 9. Name sessions with the compaction model when one is set

Hermes `agent/auxiliary_client.py` runs titles, summaries, and compaction on
a side model with its own timeout.

CA already has `agent.compaction_model` and `compaction_model_choice` in
`src/context.rs`. `start_session_title_generation` in `src/main.rs` calls
`resolve_provider_route` and names the session with the live model. A local
fallback title already exists in `automatic_session_title` (`src/session.rs`).

When `agent.compaction_model` is set, use `compaction_model_choice` for the
title request. When it is unset, keep the live model. Do not add a second
auxiliary client. Hermes `session_search` returns database messages and does
not summarize them (`tools/session_search_tool.py`), so there is no summary
action to add.

## Already in CA

These Hermes surfaces already have a CA counterpart. Leave them.

- Collapsed reasoning. `show_thinking` defaults to false (`src/app.rs`).
  `render_reasoning_block` draws a one-line "Thought" row until it is expanded.
- Elapsed time on a running tool. `tool_elapsed_label` in `src/tui/render.rs`.
- Session list. Ctrl+R opens the same list as `/sessions`.
- Slash ranking by exact, prefix, then contains (`command_match_rank` in
  `src/tui/commands.rs`). Hermes no longer has `fuzzyScore.ts`.
- Structured questions. `request_user_input` on the Main profile accepts free
  text, single choice, multi-select, and confirmation (`src/interaction.rs`).
- Background processes. `AwaitJob`, `KillJob`, `WatchProcess`, and
  `StopWatcher` are model tools (`src/agent/registry.rs`).
- Threshold compaction that keeps user text verbatim (`src/context.rs`),
  with `agent.compaction_model` and an 85% default (`CONTINUITY.md`).
  Hermes micro-compaction (`agent/micro_compaction.py`) is off by default
  because every pass rewrites the cached prefix. Do not add it.
- Prompt-cache scope on the prepared request (`src/request.rs`). Hermes
  `agent/prompt_cache_boundary.py` only marks a stable scaffold inside one
  concatenated user message. Add that when CA builds such a message.
- `agent/context_pin.py` labels a configured context-window number. It does
  not pin transcript items.
- `agent/transcript_repair.py` reconciles SQLite session rows. The request-copy
  repair to port is the one in item 5.
- Learning, Dream, Distill, and the skill curator. Hermes still describes
  skill self-improvement in `README.md`. CA already has that loop.
- Queued follow-ups, tool-run folding, plan mode, hooks, MCP, sandbox,
  dashboard, fork/rewind, `/btw`, doctor, direnv, the email gateway, voice,
  personality, memory, and session search.

## Do not port

- An Ink or TypeScript UI, or a Python gateway in front of CA.
- Hermes source copied into `src/`.
- Nous Portal, billing, credits, the pet, the desktop GUI, computer-use, and
  video generation.
- Telegram, Discord, Slack, WhatsApp, and Signal adapters. CA already has
  email.
- Modal, Daytona, Vercel, and a second sandbox family.
- A kanban board beside the agent graph.
- A local or uploaded `/insights` rollup.
- The similarity edit strategies in item 2.
