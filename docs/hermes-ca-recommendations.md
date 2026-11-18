# Hermes recommendations for CA

This file is a comparison. The newline floor is in the CA tree. The do-not-port list stays out.

Rechecked `origin/main` at `05348a3094a2356b9a271a2f22cdd6711c9952b8`
(2026-11-18T08:52:55-06:00) on 2026-11-18T16:16:23Z. GitHub `refs/heads/main`
is that same commit. CA is 0.1.184. The previous oracle commit was
`0922303631b7ee4ae9a1c3512eab8002da153888` (2026-11-18T05:01:09-08:00). The
checkout is shallow. No Hermes source was copied into `src/`.

The range is 15 files, +412/−99. The product change is the installer and the
desktop session store. `hermes_cli/source_completion.py` waits up to 30 minutes
for a live update lock, and still refuses at once when the holder recorded a
reason. `hermes_cli/web_server_sessions.py` opens one persistence reader per
browse and applies additive schema catch-up, or returns HTTP 503 when a
gateway owns the database. CA sessions are JSON. That wait and that browse are
not CA work.

## Recommendation

No open gap from this pin.

Landed in CA 0.1.184, in `split_command_segments` (`src/parity_backend.rs`). An unquoted
newline or CR is its own segment, and `command_floor` scores that segment. A
newline inside quotes stays put. A newline inside `$(...)` or backticks is
still a boundary, including inside double quotes. A literal heredoc body that
`cat` or `tee` only copies is blanked first. A backslash-newline stays one
command. The existing floor is unchanged.

## Already in CA

Leave these alone.

- Busy submit, the child dock, the transcript window, the goal footer, OSC 52, precise edits without similarity, Rust `execute_code`, image retirement, the repetition stop, one empty-reply retry, the direct check, the coding schema omitting email and image tools, compaction-model titles, and credential-path refusal.
- Shell floors that peel `command`, `env`, and `nice`, ask for `bash -c` and command substitution, and deny `LD_PRELOAD` and `PATH` overwrites. An unquoted newline is one of those segments.
- The 0.1.183 set: priced prompt-cache warmer that stops on the first cache write (`src/prompt_cache_warm.rs`); cold-request tool-body offload (`src/context_pruning.rs`); hidden characters on the approval card (`src/approval_display.rs`); device and proc path refusal (`src/device_paths.rs`); a fresh full read before `write_file` (`src/write_baseline.rs`); MCP form elicitation through the question UI (`src/mcp_elicit.rs`); opt-in rate-limited child resume, twice, default off (`src/child_resume.rs`); one image shrink after a provider size rejection (`src/image_content.rs`); a fallback line that leads with the primary sign-in (`src/fallback.rs`).
- Collapsed reasoning, elapsed time on a running tool, the session list, slash ranking, structured questions, background jobs, `watch_process`, threshold compaction with verbatim user text, plan mode, hooks, MCP OAuth, the sandbox, the dashboard, fork and rewind, `/btw`, doctor, direnv, the email tools, voice, personality, memory, and session search.
- Learning, Dream, and the skill curator. Leave the curator on its current manual switch.
- A configured context window already wins over the discovered window (`src/provider_contract.rs`). A "(pinned)" label is not a second source of truth.
- Hook `additional_context` is copied into the turn prompt and then cleared (`src/main.rs`). It does not ride later turns.

## Checked, and not a CA change

- `/fast` service tiers (`agent/fast_mode.py`). Anthropic keeps a separate prompt cache per speed, so a fast window rewrites the prefix the warmer just paid to keep.
- MCP `sampling/createMessage` (`tools/mcp_tool_sampling.py`). Hermes marks it deprecated under MCP 2026-07-28 (SEP-2577) and says not to grow it. CA advertises form elicitation only.
- Escape-drift correction (`tools/escape_drift_autocorrect.py`). It prepares text for fuzzy matching. Similarity edits stay out.
- A second empty-reply policy (`agent/empty_response_guard.py`). CA already retries an empty reply once, then stops.
- Silent `process_manage` polls (`tools/process_registry_wait_policy.py`). CA caps `await_job` and names a finished job once (`src/background_reminder.rs`). Long jobs use `watch_process`.
- Context-window suffix text (`agent/context_pin.py`), compression idle deadlines (`agent/conversation_compression_deadline.py`), the iteration counter (`agent/iteration_budget.py`), host interrupt scopes (`agent/interrupt_scope.py`), and the ESTOP sentinel (`agent/estop.py`). CA already has `max_turns`, per-schedule pause, and turn cancel.
- Batch clarify settlement (`tools/clarify_batch.py`) and desktop MCP liveness (`tools/mcp_liveness.py`).

## Do not port

- Hermes source copied into `src/`.
- An Ink or TypeScript UI, or a Python gateway in front of CA.
- Nous Portal billing, credits, the pet, the desktop GUI, computer-use, and video generation.
- Telegram, Discord, Slack, WhatsApp, and Signal adapters.
- Modal, Daytona, Vercel, and a second sandbox family.
- A kanban board beside the agent graph.
- A local or uploaded `/insights` rollup.
- Similarity edit strategies (`block_anchor`, `context_aware`).
- Micro-compaction. It rewrites the cached prefix on every pass.
- The provider filter. CA 0.1.181 removed it. Requests send real paths and command text. The `redact_*` log scrubbers are that filter under another name.
- The skills-guard regex catalog (`tools/skills_guard.py`, `tools/threat_patterns.py`). A third-party skill install can grow its own check later. Copying the pattern list is not that check.
- Plugin-catalog pins, contributor mailmap entries, and the hosted-room gateway.
- The installer update-lock wait and the SQLite session-browse catch-up from this pin.
