# User guide

## Interactive work

Start `ca` in a project directory or pass `--workspace PATH_OR_NAME`. The composer
accepts prompts and `@path` attachments. Use the application's help/command menu
to discover slash commands and current keyboard controls. Modes and tool catalogs
are displayed as part of the active conversation; verify the selected mode before
requesting a change.

Agent work can use authorized tools. Ask and Plan provide narrower workflows;
`--mode ask` and `--plan` select those launch modes. Plan review supports a durable
plan and feedback before execution. Changing mode affects the next request's
catalog; an already-frozen retry retains its original authority.

During active work, new input can steer or interrupt a turn. Ordinary leading
slash paths are accepted as prompt text when they do not match a real command.
Cancellation, transport errors, and partial output are preserved rather than
silently treated as a successful task. Inspect `/errors` for bounded local
failure details. See the detailed engineering index for exact recovery contracts.

## Sessions and dashboard

```sh
ca --continue
ca --resume-list
ca --session SESSION_ID
ca --name "Repository review"
ca dashboard
```

Sessions are durable and profile-scoped. Resume accepts the documented session
identifier forms. Concurrent processes use writer ownership to protect saved
history; a busy writer is a reason to wait or switch sessions. Session branches
and handoff previews preserve provenance and require their own acceptance.

`--no-session` keeps this run and its branches in memory. To seed a run from a
portable export without importing it into saved storage, use
`--conversation-history-file FILE` with a bounded `ca.session-export.v1` file.
The dashboard shows active and saved-session information; observing another
session does not grant authority to modify it.

## Headless use

```sh
ca --prompt "Summarize the source layout" --auto-review
ca --prompt "Summarize the source layout" --auto-review --output-format json
ca --prompt "Summarize the source layout" --auto-review --output-format jsonl
ca --prompt "Summarize the source layout" --output-last-message result.txt
```

Text is the default output format. JSON aggregates the result; JSONL emits a
stream of records. `--output-last-message` atomically writes the terminal
assistant message. `--show-thinking` includes reasoning events when supported.
Treat provider completion and application success as distinct from verifying a
requested external side effect.

`--tools`, `--include-tools`, and `--exclude-tools` control model-visible tools.
Apply them deliberately: a task requiring a hidden tool cannot execute that tool.
`--allow-network` permits approved network-capable headless tools. `--auto-review`
and `--yolo` are mutually exclusive; the latter bypasses interactive approval
prompts while other boundaries still apply.

For result validation, `--output-schema FILE` requires a bounded local JSON
Schema. `--check` requests fresh-context verification after successful headless
work. `--check-repair` permits proposed repairs through normal tool policy.
`--check-converge` additionally requires two explicitly configured isolated
reviewers; it does not configure reviewer routes for you.

## Workspace and extensions

`--add-dir PATH` adds process-only workspace roots. `--worktree[=NAME]` creates a
registered worktree; `--worktree-base REF` chooses its base. Repository setup is
reviewed with its digest, and `--skip-worktree-setup` skips it. Git metadata is
required for Git/worktree operations.

Skills, MCP, LSP, hooks, plugins, and managed local runtimes are optional.
`--skill ID` selects an exact filesystem skill; `--plugin-dir DIR` loads a
validated plugin for this process. Shell integration is available through
`ca shell-integration zsh` and explicit install/uninstall commands.

## Service and editor integration

`ca serve --stdio` exposes the versioned JSONL service. A loopback service and
`ca attach ws://IP:PORT` support authenticated local bridging. `ca acp` exposes
stable ACP v1 stdio interoperability. These modes retain CA authority; a transport
connection does not establish permission to execute arbitrary work. See
[SERVICE_PROTOCOL.md](SERVICE_PROTOCOL.md) for the wire contract.
