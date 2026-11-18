# Feature map

Reach a command with `/<name>`. Rows follow `COMMANDS` order.

/model	Switch model	[model-id] [--once]	Always
/provider	Manage provider templates, providers, and models	[templates|setup|discover|…]	IdleOnly
/preset	Apply a named route preset	[name]	IdleOnly
/preset-save	Save the current route as a named preset	<name>	IdleOnly
/add-dir	Grant an additional session workspace root	<path>	IdleOnly
/workspace	List or manage named workspaces and session roots	[list|save <name>|load <name>|delete <name>|cd <path>|remove <root-id>]	IdleOnly
/save-workspace	Save the current ordered workspace roots by name	<name>	IdleOnly
/load-workspace	Load a named workspace into the current session	<name>	IdleOnly
/rule	Browse and manage conditional user or workspace rules	[filter|view <id>|activate <id>|deactivate <id>|create …|edit …|delete <id>|confirm <digest>|reload]	IdleOnly
/command	Browse and manage native custom prompt commands	[filter|view <name>|create …|edit …|delete <name>|confirm <digest>|reload]	IdleOnly
/plugin	Browse and manage verified local plugins and signed catalogs	[list|search <query>|show <id>|add <manifest>|enable <id>|disable <id>|remove <id>|catalog <action>]	IdleOnly
/direnv	Preview and approve a process-only workspace environment overlay	[confirm <sha256>]	IdleOnly
/skills	Search and manage filesystem skills	[query|activate <id>|deactivate <id>|view <id>|unavailable|config <selector> [<key>=<value>] [--workspace]|reload]	IdleOnly
/bundles	List, inspect, create, delete, or reload task-local skill bundles	[show <name>|create <name> <skill1> <skill2> [...skill5] [--workspace]|delete <name>|reload]	IdleOnly
/recall	Search privacy-filtered completed CA, Codex, Hermes, and MiMo sessions locally	[query|read <session-ref> [cursor]]	IdleOnly
/learning	Inspect the metadata-only learning and reusable-asset lifecycle	[skills|memory|runs|graph|suggestions|skill <selector>|<event-id>]	IdleOnly
/mcp	Inspect, authorize, or reconnect configured MCP servers	[verbose|connect <server>|refresh <server>|disconnect <server>|reconnect <server>|trust-project [--network]|allow-network <server>]	IdleOnly
/hooks	Inspect, test, and toggle lifecycle hooks	[filter]	IdleOnly
/permissions	Inspect permission rules and recent decisions	[filter|why]	IdleOnly
/memory	Inspect and manage opt-in user or workspace memory	[filter|export <path>]	IdleOnly
/learn	Learn directly from recent work and publish validated reusable assets	[focus text|status [run-id]|sync|undo <run-id>]	IdleOnly
/personality	Answer guided questions to customize how CA works and communicates		IdleOnly
/dream	Manage conservative memory maintenance	[run|status|on|off|undo <run-id>]	IdleOnly
/distill	Distill reusable assets directly without changing memory	[focus text|run [focus]|status|on|off|undo <run-id>]	IdleOnly
/goal	Set or manage a finite evidence-evaluated goal session	[status|pause|resume|clear|<condition> [--iterations N] [--tokens N] [--minutes N]]	Always
/workflow	Run, inspect, or manage finite declarative and audited built-in workflows	[templates|inspect builtin:<id>|run <path|builtin:id> [options]|tree [builtin:<id>]|status|pause|resume|clear]	Always
/automation	Inspect local automations and review an exact definition before enabling it	[list|inspect ID|history [ID]|pause ID|resume ID|run ID|result ID|preview FILE|create TOKEN|update ID TOKEN]	Always
/worktree	Create, inspect, attach, or clean up isolated Git worktrees	[list|create <label> [base]|status <id>|diff <id>|attach <id>|cleanup <id>]	Always
/multi-run	Run one request in isolated candidate worktrees	[2-5] [request]	IdleOnly
/reasoning	Show or select configured reasoning effort	[none|minimal|low|medium|high|xhigh|max]	Always
/thinking	Toggle expanded reasoning display	[on|off]	Always
/plan	Plan, hand off across providers, inspect the workflow, or execute	[show|run <request>|send [review|revise] [agent|provider/model]|graph|pause|resume|cancel|execute [provider/model|choose]|approve|revise|dismiss|reject|edit <title>|recipes]	IdleOnly
/recipes	Review learned Rust recipes without a model request	[server] [recipe-id]	IdleOnly
/self-improve	Open machine learning, evaluation, deployment, pause, and rollback controls	[server]	IdleOnly
/tasks	Manage dependency-aware plan tasks and child agents	[status|list|start|…]	IdleOnly
/agents	Inspect and control asynchronous child agents	[pause|resume|cancel <id>|message <id> <text>|approve <id>|deny <id>]	IdleOnly
/ask	Enter ask mode		Always
/btw	Ask an ephemeral side question	[question]	Always
/voice	Record voice into an editable draft		IdleOnly
/debug	Enter debug mode		Always
/logs	View safe runtime diagnostics	[level=<info|warn|error>] [component=<name>] [event=<name>]	Always
/errors	View recent safe provider failure categories	[dismiss]	Always
/support-bundle	Preview and create a private metadata-only support archive	[since=<duration>]	IdleOnly
/yolo	Auto-approve all tools		Always
/auto-review	Toggle safe-tool auto-review		Always
/mode	Switch to a named agent mode	<name>	IdleOnly
/new	Start a new session; keep the previous one		IdleOnly
/clear	Clear the visible conversation		IdleOnly
/quit	Quit ca		Always
/help	Show command help		Always
/vim	Toggle Vim input mode		Always
/version	Show ca version		Always
/config	Search and edit settings/configuration	[filter|explain [selector]]	IdleOnly
/sandbox	Inspect process isolation and choose the next-launch sandbox mode		IdleOnly
/context-limit	Edit model context window and checkpoint threshold	[provider/model]	IdleOnly
/prompt-cache	Inspect or edit prompt caching for one exact model	[provider/model]	IdleOnly
/usage	Show context, account, and session usage in a tabbed dashboard		Always
/dashboard	Open the agent dashboard		Always
/request	Inspect the latest prepared model request		Always
/trace	Inspect or control persistent provider exchange tracing	[on|off|list|open <id>|delete <id>]	Always
/filter	Report that the filter was removed		Always
/keymap	Inspect or edit global keyboard shortcuts	[filter]	IdleOnly
/history	Search process input history	[filter]	Always
/stash	Save or restore explicit composer drafts	[save]	Always
/feedback	Record explicit feedback for this task	accepted|rejected|unknown	IdleOnly
/doctor	Check local configuration and runtime readiness		IdleOnly
/audit	Audit local agent-harness security boundaries		IdleOnly
/raw	Open a copy-friendly transcript in native scrollback		IdleOnly
/output	Rerun the latest semantic command with full native output	full	IdleOnly
/scratch	Manage project notes, todos, and saved plan references	[note <text>|todo <text>|edit <id> <text>|toggle <id>|plan|delete <id>]	IdleOnly
/copy	Choose a prior message part to copy	[last|all]	Always
/diff	Review working-tree changes		IdleOnly
/walkthrough	Explain one complete bounded Git snapshot	[working|base <branch>|commit <sha>]	IdleOnly
/review	Run a fresh-context model review or explicit dual-route convergence	[working|base <branch>|commit <sha>|custom <instructions>|converge [--ack-unpriced] [SCOPE]]	IdleOnly
/acceptance	Create or inspect immutable convergent-review acceptance packets	create|latest|list|export <FILE>	IdleOnly
/artifacts	Browse and inspect the current session's tracked files	[search <query>|inspect|view|copy|untrack <number-or-path>]	IdleOnly
/split	Open a bounded horizontal or vertical session pane	horizontal|vertical [new|SESSION]	Always
/pane	Focus, close, or list interactive session panes	next|prev|close|list|<number>	Always
/save	Save the current session		IdleOnly
/sessions	Browse and continue previous sessions	[session-id]	IdleOnly
/retention	Preview and manage saved-session retention		IdleOnly
/export	Export the current session conversation	[markdown|json] [path]	IdleOnly
/import	Import portable or inert foreign session history	[codex|claude|opencode] <path> | setup claude <directory>	IdleOnly
/init	Draft root AGENTS.md with review		IdleOnly
/compact	Compact conversation context	[focus] | here [N]	IdleOnly
/summarize	Show context estimate		Always
/shell	Start a direct shell command		IdleOnly
/commit	Review staged changes and commit		IdleOnly
/undo	Undo the last tracked file mutation		IdleOnly
/redo	Redo the last undone file mutation		IdleOnly
/recover	Preview and restore durable recorded turn edits	[list|preview TURN_UUID|restore TOKEN]	IdleOnly
/branches	Browse local session branches and archived relatives	[page OFFSET|switch SESSION_UUID]	IdleOnly
/bookmark	Save a named completed message boundary	[list|add LABEL|rename UUID LABEL|delete UUID|branch UUID]	IdleOnly
/pause	Pause the active turn at its next safe boundary		Always
/unpause	Resume a turn paused at a safe boundary		Always
/fork	Fork the current conversation	[title]	IdleOnly
/rewind	Rewind the last conversation exchange		IdleOnly
/retry	Retry the failed or last user request in a new branch		IdleOnly
/timeline	Browse completed conversation turns		IdleOnly
/queue	View and edit queued messages		Always
/tools	Choose the coding or full tool schema	[coding|full|status]	Always
/context	Show context usage and checkpoint coverage		Always
/session	Show conversation history and current-sitting metrics		Always
/jobs	View foreground and background shell jobs		Always
/delegate	Show codex-team delegation readiness or recover one durable delegation job	[JOB_ID]	Always
/evidence	Explain mechanistic preflight, verification, diagnosis, and cache evidence	[JOB_ID|metrics]	Always
/term	Manage durable tmux terminals	[filter]	IdleOnly
/theme	Select a built-in terminal theme	[filter]	IdleOnly
/sync-theme	Detect the current terminal background again		Always
/status-indicators	Configure terminal-title status indicators	[on|off]	Always
/statusline	Configure statusline fields and custom status		IdleOnly
/compact-mode	Toggle compact mode (reduced padding)		Always
/line-numbers	Toggle code line numbers		Always
/reload	Reload project instructions		IdleOnly
/name	Name the current session	<title>	Always
/connect	Sign in to OpenAI Codex via browser OAuth		IdleOnly
/disconnect	Sign out of OpenAI Codex and revoke tokens		IdleOnly
