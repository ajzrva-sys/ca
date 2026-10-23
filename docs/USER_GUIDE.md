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

## Teams and final review

Ordinary Agent requests proactively use focused helpers when independent work
will save time or improve coverage. Helpers inherit the parent route and frozen
project/personality instructions, with narrower permissions. Ask, Plan, bare,
restricted roles, and explicitly paused spawning keep their existing limits.
After authorized, substantive reads across two source components, CA starts one
read-only Explorer if the turn has no ordinary helper yet. The parent can keep
working while the Explorer investigates; CA returns its findings to the parent
before final review. Small, single-component requests do not force this helper.
`[agent] proactive_delegation = false` opts out of both the guidance and this
automatic exploration.

The expandable `Subagents N` roster shows each helper's role, task, actual model,
phase and elapsed time. Completed durations stop changing. Use Ctrl+X, then
Down to focus the roster. Enter opens a helper's details; Esc returns to the
conversation. Saved sessions retain completed helper summaries and durations.

Display roles such
as Reviewer are separate from executable agent types (`explore`, `plan`,
`general`). Source, test and runtime configuration changes require independent
read-only review before ordinary Agent completion; this includes child and shell
edits. Review is bound to the exact candidate, reused only while unchanged, and
remains incomplete when denied, paused or unavailable. Documentation-only and
no-op changes skip it. `[agent] review_changes = false` explicitly disables the
host review barrier.

Helper findings and review reports return as evidence. They do not change your
instructions or permissions. After review approves the candidate, the parent
incorporates that report into its final response.

A parent can control helpers on another configured provider. For example, a
DeepSeek parent can delegate a task to a configured OpenAI or native Codex route
while keeping its own model selected. Ask CA to use the provider and model you
want for the helper. CA can discover exact configured identities with
`list_agent_routes` and select a child `route` on `spawn_agent`, each
`spawn_agent_batch` task, or `task`. It then uses the same `list_agents`,
`await_agent`, `send_agent_message`, and `cancel_agent` controls across providers.
The returned helper snapshots include their provider and model.

An exact child route bypasses automatic price routing. A configured route still
needs working credentials and provider availability; listing it sends no probe
and does not verify authentication. If no exact route is selected, helpers keep
the inherited or configured delegation default. Ask and Plan retain their tool
limits. Use `/agents message ID TEXT` or `/agents cancel ID` to steer or cancel a
running helper yourself.

Repository-query and verification tools use the shared local codex-team engine
without an MCP setup step. Explicit external backends retain their binding.
Ordinary filesystem tools retain untracked and non-Git scope.

## Cross-provider plan handoffs

In a persistent interactive session, ask CA to send a plan between named agents
on configured providers. Agents can author, review and revise the plan, return it
to an earlier participant, and implement it on a separately selected route. For
example:

```text
/plan run Create a plan on A, review on B, revise on A, then implement on C
/plan send review provider/model Check failure handling
/plan send revise author Apply the feedback
/plan execute provider/model
/plan graph
```

Use configured provider/model names in the request. Named participants retain
their selected route; returning to one starts a fresh request with its previous
contributions, the latest plan and feedback. Planning agents receive read-only
tools. A planning-only request stops after planning; a request that includes
implementation continues with normal Agent tools and permissions. Review loops
default to three review passes and pause if the plan is still unaccepted.

`/plan send` opens the provider picker when no recipient is supplied.
`/plan execute choose` selects the implementation route. In `/plan graph`, arrows
select a visit and Enter opens its saved plan and feedback. Tab shows previous
agents; Enter drafts a return to the selected agent. S selects a recipient and
drafts a handoff; edit its review/revise role and instructions before sending.
E selects an
implementation provider, P pauses or resumes, C cancels, and Esc closes the view.
The handoff graph is separate from the implementation dependency graph in
`/tasks`.

CA saves every handoff and immutable plan revision. Shareable `plan.md` and
revision Markdown files live under the active profile's
`plans/<session-id>/<workflow-id>/` data directory. Restored workflows remain
paused when they had active work; resume them explicitly.

## Providers and models

Open `/provider`, select a connected provider and press Enter to browse its
models. Enter on a disconnected built-in provider opens connection setup;
Shift+L reconnects it. Type to filter, use the arrows to highlight a model,
and press Enter
to select it. Shift+R refreshes the highlighted provider from either the
provider list or its model detail. `/model` browses models across providers;
Shift+R refreshes the full list. Filters remain unchanged; refresh preserves
the highlighted model when it remains in the catalog.

The native Codex route fetches your account's current visible models and
supports GPT-6.1 Sol when your account offers it. `/model` discovers models
across providers even when one provider's refresh is already running.
Live/cached/fallback status describes the catalog source, rather than whether
an inference request has succeeded. Relaunch CA after an executable update.

## Personality and continuous behavior learning

Open `/personality` inside CA, or run `ca personality` from a terminal. Choose
the eight-question quick setup, the full 48-question walkthrough, or one of
12 behavior sections. Each question has explained choices, custom text, and
a skip option. Go back to
adjust answers, then review and save. Saved answers reopen automatically;
you never need to edit a file.

CA uses saved preferences on the next ordinary turn or headless request.
An active turn and its retries retain their existing steering. Named profiles
keep separate preferences. Setup works without a provider or model.

`ca harness questionnaire show` lists 48 audit questions. Keep
`ca harness questionnaire watch --interval 300` running to learn investigation
priorities from new recorded outcomes. Unchanged cycles stay quiet; stop with
Ctrl+C and inspect retained results with `ca harness questionnaire status`.
Observed signals are investigation evidence, and unsupported answers remain
unknown. See the [questionnaire guide](HARNESS_QUESTIONNAIRE.md).

Continuous knowledge learning uses one codex-team supervisor and CA's durable,
learning-only scheduler queue. Harvesting is local; provider work advances a
resumable evaluation stage. The shared allowance is $2 per Eastern calendar day
across background learning and harness experiments. Each physical request
reserves its upper bound before dispatch. Unavailable pricing blocks admission;
missing usage retains the reservation through restart. Budget-paused work can
resume after midnight in America/New_York.

Evaluated candidates await exact adoption review. The inbox holds eight;
unanswered proposals do not monopolize the active evaluation slot. Memory,
skills, permanent steering and code all require approval bound to the candidate,
evaluation, evidence and current destination baseline. Manual learning stages
proposals through review too. `ca learn service status` shows operational state
and budget. `ca learn service refresh` previews a digest-bound repair of stale
enrollment/profile/binary/service bindings with recovery copies; apply its exact
current confirmation digest only after reviewing the preview.

The Default theme uses charcoal panels, shaded user prompts, readable gray
activity text, purple agent identities and green completion markers. Other
themes remain selectable in UI settings.

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

## Context reduction, formatting and service input

CA automatically omits eligible older tool-result bodies from model requests while retaining complete canonical history. `/context` reports how many results were omitted and the estimated token savings. Use session search or recall when older evidence is needed. Disable this with `[context_pruning] enabled = false`.

Optional formatter presets and custom commands transform native edit candidates before approval, recovery publication and LSP diagnostics. Enable them in configuration and authorize the formatter command policy. Formatter failures remain visible and keep the requested unformatted edit.

Service clients can use `submit_input` to steer at the next safe boundary or queue work for the next idle boundary. Choose `start_persistent_session` to save history and inbox state, then `load_persistent_session` and `resume_inbox` to resume pending work explicitly. The ordinary ephemeral service remains the default. See the configuration guide and service protocol for interfaces and limits.
