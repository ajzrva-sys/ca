# Architecture

## AUT-259 automation runtime ownership

`automation.rs` retains V1 definitions, occurrences, budget evidence and session
delivery. `schedule.rs` admits legacy and automation records in due-time/ID
order. `automation_runtime.rs` supervises profile-scoped durable worker runs,
holding shared resource ownership through descendant cleanup. It invokes ca's
native agent runner for ordinary work and supplies provider/session services
without introducing an engine dependency on ca.

`ca-automation-protocol::execution` defines additive V2 run authority; V1 wire
identities remain intact. `ca-automation::{runtime,native,repository}` supplies
bounded execution, action reconciliation, typed adapters and verified local
publication. `repository_automation.rs` supplies isolated ca workers and exact
candidate review. `harness_automation.rs` supplies native target admission and
common physical-request budgets; promotion remains behind exact review.
`monitor_automation.rs` owns observation only. The `automation-engine` CLI is a
provider-free, digest-confirmed facade and compatibility migration entry point.
See `docs/CA-automation-runtime.md` for interfaces and storage.

## ORC-258 session ownership, durable edits and runner boundaries

`session_access.rs` keys writers by canonical profile storage and session UUID.
Stable OS advisory files and in-process admission protect active turns,
heartbeats and short metadata writes. `session.rs` validates UUID, canonical
history generation/digest and metadata revision before publication. An idle
pane reloads a changed committed history before dispatch; changed workspace
roots require an explicit reload. Read-only presence/dashboard inspection
does not acquire writer ownership. Cloned writer capabilities share authority;
the last owner in the originating process explicitly unlocks the stable file so
a descriptor copied into a pre-exec child cannot delay release. A PID guard keeps
forked child destruction from releasing the live parent's authority.

Automatic response replacement reserves at most one frozen request before
side effects. The failed-turn completion saves its source through the active
writer and updates AppState's committed binding before a background worker
writes only a new branch. Source-save failure stops replacement; branch failure
or cancellation cannot advance the original behind its in-memory binding.
Local save-stage/error details are terminal-sanitized, capped at 96 characters,
and retained in the pane's bounded `/errors` history without provider bodies.

`owned_tools.rs` carries session/parent-turn attribution into typed native
mutations. `recovery.rs` syncs bounded before/after blobs and private versioned
intents before application, reconciles exact interrupted states without
restoring them, and records restore/undo transactions with rescue provenance.
Workspace activity/restore leases also protect overlapping roots across
profiles and processes. Shell jobs retain activity ownership through process
and output cleanup. Recovery bytes never enter transcripts or portable exports.

`plan_feedback.rs` anchors notes to logical Unicode ranges or task identities
and a content digest independent of task progress. `session_navigation.rs`
queues bounded worker actions; `branches.rs` derives family pages and prefix
bookmarks. `branch_handoff.rs` generates one accounted tool-free context
summary; preview acceptance validates both stored snapshots before switching.
Source grants, queues and verification do not transfer with that context.

`dashboard_snapshot.rs` combines immutable all-pane/child and saved-session
rows using owner identities. `dashboard_presence.rs` publishes private bounded activity
metadata, verifies process-start identity and refreshes external records on
workers. External rows expose observation and labeled committed previews only.
`hooks.rs` isolates session/pane/turn/generation/child outcomes, with separate
invocation queues for overlapping lifecycle calls. Compaction and child events
run outside manager locks and preserve committed outcomes on notification error.

`tool_discovery.rs` advertises authorized native extensions after complete
result-batch commit, with request-boundary eviction and frozen retry schemas.
`agent/registry.rs::ToolConcurrency` makes native-read eligibility explicit;
runner batching preserves order and serial approval/hook/mutation barriers.
Bounded native query workers reject escaping candidates, avoid symlink
recursion and stop cooperatively when their result owner is cancelled.

`memory.rs` separates deterministic ranking from rendering. Optional local
hybrid recall uses the managed `local_models.rs` Embeddings mode, an explicitly
registered runtime/model, native-record derived indexes and lexical fallback.
`memory_benchmark_cli.rs` evaluates offline fixtures or explicitly selected
local models. `automation_dispatch.rs` reserves bounded requests/tools before
dispatch; `automation.rs` records frozen results before exactly-once session
delivery. `automation_ui.rs` stages definition previews and explicit one-use
confirmation, while `automation_notifications.rs` admits bounded native alerts
once and uses verified presence for focus suppression.

## BUG-257 responsive terminal and bounded replacement

`tui/event.rs` queues only real terminal input, timestamps dequeue latency,
and supplies skipped-missed-tick intervals separately. Ordered key/paste input
has priority with a 32-event fairness slot for timers/draws. Resize/drag events
coalesce for at most 16 ms. Deferred and immediate draws share the same 16 ms
frame limit. `terminal_awareness.rs` reads only safe `COLORFGBG` hints; theme
sync keeps cached/default appearance if absent and never directly reads the
terminal input stream. `main.rs` avoids unchanged idle redraws.

`tui/transcript_cache.rs` belongs to `tui/render.rs`. Each pane caches styled
128-source-line blocks, wrapped-row prefix indexes and markdown on workers;
warm frames binary-search and paint only visible rows. Session/generation,
width, theme, focus/expansion and content revision key invalidation. In-flight
results from incompatible owners are discarded while compatible old history
stays visible. The steady styled payload budget is 16 MiB layout +16 MiB
markdown per pane; metadata, canonical history and transient worker buffers
are not an overall RSS cap. Evicted rows can be rebuilt without data loss.
Cursor blink and current plan/approval controls overlay the cached body.

`idle_learning.rs` polls background, read-only maintenance admission at most
once a second. `harness_control.rs::PricingAdmissionBlocked` carries typed
route/profile/catalog-revision identity; `harness_idle.rs` suppresses repeated
unchanged failures and re-arms on those identities changing. `pricing.rs`
hashes a bounded catalog read for revision (including same-length rewrites).
Missing verified prices never change ordinary chat eligibility.

`turn_recovery.rs` owns private per-logical-turn reservations and a bounded
20-entry sanitized failure ledger; `main.rs` captures the exact prepared
request/client before transport. Retryable transport/EOF or 429/selected 5xx
failures can reserve one replacement only before tool/hook effects, outside
workflow/goal/approval lanes. The source snapshot is persisted before the
replacement branch is exposed; it retains failed partial output, while the
branch reuses the original user request/attachments without the partial
assistant. Replacement uses one-attempt transport and does not rerun request
preparation, hooks or workspace setup. Delay cancellation and stream ownership
use generations; a second failure remains manual. `/errors` exposes safe
route/category/resolution metadata, not provider bodies or credentials.
`runtime_diagnostics.rs` records bounded input-age and replacement events
without key/paste/prompt contents. Neither these diagnostics nor cache payloads
change canonical session formats.

## TUI-256 Grok-style usage-limit graph

The Usage limit tab paints a compact title, two-tone `█`/`░` meter, percent,
and caption. `provider_usage::usage_limit_graph` derives those from the
configured provider report (MiMo token-plan ratio, DeepSeek remaining,
OpenAI month-to-date) plus a one-line process ledger. Unsupported providers
are omitted. `/usage` still opens the three-tab modal; the full dump stays on
the leftover pager path.

## TUI-255 Grok-style agent dashboard

`src/tui/dashboard.rs` owns the full-screen agent roster and the tabbed usage
modal. `/dashboard`, `Ctrl+\`, and `ca dashboard` open it. Rows are the current
conversation (Working/Idle) plus other saved sessions (Inactive). `/sessions`
stays the saved-session picker (Open Previous). `/usage` opens Context /
Usage limit / Session info over the dashboard or conversation. Dispatch with a
prompt starts `/new` when the current session has content, then submits.

## BUG-242 Astra capability boundary

`provider_contract.rs` recognizes exact gpt-6-astra on official Responses routes.
Its profile enables five API reasoning efforts and excludes none. Existing
profile precedence, selection and request paths remain shared. `ReasoningEffort`
serializes XHigh as xhigh and accepts legacy x_high when reading stored data.


## PLN-241 plan review and execution boundary

`src/plan.rs` owns process-local `PlanReview` ID/revision bindings, reader and
revision controls, transactional approval, and a single event-loop execution
request. Planning completion ends the provider epoch before displaying review;
Execute saves approval and begins a fresh Agent turn, rebuilding instructions
and canonical tools. Main-agent authority comes from normal tool/workspace
policy, independently of optional graph grants. Active graph work prevents
concurrent execution. Explicit execution hydrates paged saved history first.

`PlanArtifact.body` is optional Markdown with legacy task-list fallback;
material content edits revoke approval while task progress does not. Typed
proposal mutations alone offer execution. Session application clears review
and pending execution, so restoration is inert. The TUI projects an inline
card only during idle pending review (PLN-244), with all tasks and complete
wrapped entries (PLN-243), plus a full Markdown
reader with visual-row scrolling and existing selection/copy behavior;
diagnostic tool output stays available on expansion.

## TUI-240 stop and interrupt controls

Busy composer Enter produces `InputAction::Steer`; the configured queue action
(Alt+Enter by default) calls `queue_composer_draft`. Editing a queued draft uses
Enter to send now or the queue binding to save/release that draft for later.
`prepare_interrupt_submission` stops the old turn, keeps validation failures
recoverable, and routes the replacement through ordinary attachment resolution
and `continue_tui_submission`, including prompt hooks and image handling.

Composer Esc produces `InputAction::StopTurn` after focused popup handling.
Ctrl+C/Ctrl+G stop active turns before other busy handlers; approval/interaction
Esc shares that path. `interrupt_active_turn` aborts the owned request/tool/
approval/evaluator tasks and immediately advances the stream generation. It
clears pending interactions and autonomous continuations and holds existing
queue IDs behind the process-only `restored_queue_ids` gate. Late old-generation
messages cannot resume work; held drafts require explicit sending or requeueing.
`stop_active_turn` records the stopped state without dropping composer contents.

`jobs::ForegroundWaitGuard` ties a foreground command to its awaiting task.
Dropping that waiter requests owner-checked process-group cancellation through
the existing job manager. Promotion atomically transfers responsibility to the
background lifecycle; the guard cannot kill promoted or foreign-owner jobs.
Completed edits are not rolled back. The composer has a persistent controls row
separate from temporary status messages; held queues are labeled `On hold`.

## BUG-239 native Codex model discovery

`model_catalog::native_codex_catalog` reuses the exact first-party route
classifier. That route requests `/models?client_version=0.153.0`, identifies
itself as CA, resolves existing CA authentication, and attaches an account ID
only when it matches the resolved bearer token. Generic/custom catalog routes
retain their existing request and decoder. Native decoding is minimal: visible
`models[].slug` only, bounded by the common timeout/body/ID/count checks.

`AppState::codex_catalogs` records only successful native catalogs (including
empty success), separately from generic/discovered fallback rows. Both refresh
handlers share `apply_native_codex_catalog`: live success replaces discovery;
failure keeps last-success IDs with cached/error status. Provider edits/deletes
invalidate this in-memory state. Configured and active session models are
unioned into picker rows, but never inserted into the live catalog. Missing
live entries get configured-only labels in both pickers. Selection restoration
uses actual recent/favorite-grouped rows. No state is persisted or remote model
instructions/capability metadata imported.

## SES-236 fresh session transition

`CommandId::New` is separate from `Clear`. Interactive dispatch sends
`InputAction::NewSession` through `prepare_new_session`, captures the shared
`current_session_snapshot`, and saves on a blocking worker. A generation-bound
`NewSessionSaved` event switches only after success. Input and autonomous ticks
are held while the snapshot commits; active foreground/background work must
finish first. Missing paged prefixes load against the exact storage generation.

`apply_session_state` shares restore/reset mechanics without reloading the
workspace for a fresh session. `/new` also clears retry, approval grants, task
loads, undo, attachments, and sitting state, creates a fresh agent manager, and
keeps the current route and workspace. The new identity is an independent root
and is saved lazily when it has content. No wire or session-format change.

## BUG-235 bounded pre-output recovery and replacement branches

`src/agent/client.rs` owns one physical-request counter across response headers
and body-stream transport recovery. Ordinary calls are capped at two POSTs;
one-attempt callers remain one-attempt. `open_provider_response` shares frozen
request bytes, bounded status delays, and cancellation across both phases.
Only transport failures, 429, and 500/502/503/504 can replay before commit.
`provider_event_has_usable_output` in `src/agent/stream.rs` is shared with the
turn/fallback boundary. Nonempty decoded text/reasoning, tool-call starts, and
nonempty tool arguments commit before text-format normalization. Attempt-local
metadata is buffered until commit or clean completion and discarded on safe
replay; request-store and logical-turn accounting remain outside that replay.

`src/main.rs::prepare_failed_turn_replacement_branch` recognizes only a failed
turn whose replay boundary points to its terminal user-only message. After
attachment resolution, prompt hooks, and task-skill validation, ordinary
submission saves the source, branches immediately before that failed message,
and appends the replacement prompt to the child. Source queues stay on the
source through the existing branch contract; bare and `--no-session` use the
same in-memory branch path. Save/branch failure restores the draft before any
provider request. Partial-assistant histories, resolved-tool continuation, and
explicit `/retry` keep their existing semantics.

Failure footers distinguish pre-output recovery, HTTP 400 rejection, and
interrupted output, while retaining authentication/setup guidance. This adds
no configuration, CLI, provider-wire, or session-format field.

## BUG-234 DeepSeek DSML and legacy Dream recovery

`src/agent/dsml.rs` is a bounded stream normalizer between provider wire
decoding and `StreamAnomalyGuard`. It activates automatically only for the
exact built-in DeepSeek V4 Chat Completions contract, or through a validated
tool-capable Chat Completions model profile with
`tool_call_format = "deepseek_dsml"`. Complete exact ASCII/full-width wrappers
become ordinary host-ID `ToolCallStart`, JSON-argument, and tool-finish events;
the existing advertised-tool, approval, sandbox, and execution paths remain
the only authority boundary. Native calls and fenced examples pass through;
malformed, mixed, oversized, prose-mixed, and non-opted-in DSML fail closed
before transcript assembly.

`src/learning.rs` owns narrow writable-entry recovery for authentic V1
supervised intents created by the historical character-count sanitizer. Under
the existing store lock it validates private no-follow storage, enrollment,
filename/ID, original digest, structure, and every safety rule; proves the sole
defect is the legacy UTF-8 byte mismatch; archives the exact source bytes under
a private digest-addressed path; then atomically publishes a re-sanitized
same-ID record with corrected byte totals and digest. Startup, harvest, and
queue entry points invoke the idempotent recovery and expose only a bounded
count.

## Inline conversation-agent worker identity

`src/tui/render.rs` derives one compact glyph/color pair from each stable
conversation-agent UUID at render time. The mapping is process-independent and
requires no new state or session field, so live `AgentActivitySnapshot` rows and
restored `TranscriptAgentSummary` rows share the same visual identity. Status
styling and focus controls remain separate from the identity color; task-graph
and multi-run origins remain on their existing dedicated surfaces.

## ORC-218 safe Claude setup import

`src/setup_import.rs` inventories a bounded regular Claude setup tree and
renders only inert instructions, conditional rules, prompt commands, and
CA-valid portable skills into native user/project roots. A versioned preview
binds canonical roots, scope, every source/rendered-target digest, exclusion,
collision state, and byte count. Apply reproduces that exact digest, stages all
private files, publishes without overwrite, synchronizes parents, and rolls
back only transaction-owned paths. CLI JSON remains read-only without an apply
digest; the TUI is Cancel-first and performs file work off the input loop.

## ORC-218 stable editor context and anchored edits

`src/acp.rs` accepts explicit stable-ACP text embedded resources and contained
`file:` links only within per-item, aggregate, media, range, regular-file, and
symlink bounds. It renders visible provenance into that prompt and never
discovers or persists editor context. `read_file.include_line_hashes` produces
optional eight-hex line-content anchors without changing legacy output;
`hashline_edit` rereads and requires one unique ordered anchor range before the
ordinary approval, workspace/sandbox, undo, receipt, LSP, loop-guard, activity,
and artifact-ledger pipeline can mutate bytes.

## ORC-218 bounded interactive panes

`src/panes.rs` owns a process-only, four-leaf horizontal/vertical layout tree
and the materialization boundary for complete pane-local `AppState` values.
Session, model/mode/workspace, composer, queue, approvals, work/goal/workflow,
job owner, agent/MCP/LSP managers, and stream generation remain pane-local;
only explicitly enumerated configuration, catalog, diagnostics, appearance,
and process stores synchronize when focus changes. Layout is never serialized
and does not affect headless, service, or ACP execution.

`src/main.rs` gives every pane a private raw stream channel and routes each
message through a `PaneId` envelope before applying the existing generation
guard. Unfocused provider work may continue, but mutating/network/unknown
executions become pending approvals until their pane is focused. Split-session
disk reads run on a blocking worker and return over that pane's channel. Pane
closure is Cancel-first, reaps only its owned turn/jobs/managers, and preserves
saved sessions; top-level teardown visits every pane. `src/tui/render.rs`
projects all leaves with a focus/status frame while only the focused leaf owns
composer input, overlays, and the terminal cursor.

## ORC-218 session artifact ledger

`src/session_artifacts.rs` owns the versioned `ca.session-artifacts.v1`
metadata ledger. It accepts only authoritative successful file-mutation
receipts plus explicitly enabled `read_file` observations, coalesces at 512
entries, and stores workspace root identity, relative path, access/count,
message boundaries, timestamps, size, and content/receipt digests—never file
content or raw workspace roots. `src/main.rs` records tool, project-init, and
undo/redo mutations; `sessions.track_reads` separately gates read recording.

The `/artifacts` surface lists/searches saved metadata and resolves a selected
entry against the current `WorkspaceContext`. Inspection and viewing reject
missing roots, traversal, every symlink component, non-regular or oversized
files, and content/size drift before showing UTF-8 text. Copying uses only the
stored relative path; untracking changes only ledger metadata. `src/session.rs`
persists the defaulted ledger and trims it to a branch/rewind message boundary;
`src/session_portability.rs` deliberately has no artifact field.

## ORC-218 active-work lifecycle

`src/keep_awake.rs` is a process-local, non-authorizing lifecycle service. A
small actor owns the platform helper child; opaque RAII leases send only
acquire/release IDs, and a pure tracker prevents underflow, duplicate cleanup,
and release/reacquire races. `AppState` owns the interactive-turn lease, while
`JobState` owns a separate lease only for `JobOrigin::ModelTool`. Terminal job
status drops its lease before notification, and top-level shutdown reaps jobs
before stopping the helper service.

`src/jobs.rs::stop_and_await_owned` retains the existing owner check and status
watcher. It transitions to Cancelling, signals the full process group
gracefully, waits the bounded grace, escalates, and snapshots only after the
watcher reaches a terminal state. `src/agent/tools.rs` reserves and finishes
the pre-existing completion-delivery marker around this operation, preserving
deduplication with automatic background completions.

## ORC-218 audited built-in workflows

`src/workflow.rs` owns three immutable `builtin:` definitions and their
parameter preflight. They remain ordinary bounded declarative DAGs: the same
route, tool, permission, evidence, retry, output, persistence, and cancellation
boundaries apply, and no executable workflow language is introduced. A
built-in run persists its validated definition alongside its canonical digest
so resume does not trust a mutable external file. Built-in identifiers are
reserved from user definitions.

`src/main.rs` instantiates built-ins asynchronously before the existing
Cancel-first preview. Fact-check juror steps use an empty canonical request
projection and receive evidence only through the workflow control prompt;
this prevents one juror's transcript from influencing another while retaining
their results for the final report. Research-experiment evaluation grants use
an exact invocation matcher bound to command bytes and the network flag, and
are checked before hooks or ordinary approval can widen them. The TUI tree
projection exposes only safe DAG/run metadata and never prompt or output text.

## CUR-204 local workspace, lifecycle, automation, and interoperability

CUR-204 adds local feature surfaces around the existing provider, permission,
session, and TUI cores; it does not introduce hosted state or a second authority
model. `src/workspace_catalog.rs` stores bounded profile-private named root sets,
and `WorkspaceContext` remains the single ordered multi-root value passed to
instructions, rules, skills, commands, attachments, MCP, LSP, sandboxing,
providers, sessions, and launch worktrees. `src/rules.rs` owns legacy/always,
glob, and manual activation plus digest-bound native mutations. Workspace
commands and skills may be hidden from automatic context without disabling
exact user activation.

`src/agent/jobs.rs`, `src/main.rs`, and TUI input state distinguish model,
direct-shell, and internal job origins. Promotion preserves the running process;
explicit await and host automatic completion share one exactly-once claim.
`src/plugin.rs` and `src/plugin_catalog.rs` keep signed/catalog validation and
collision authority while process-only plugin directories compose without a
registry write. `src/run_environment.rs` owns an immutable, redacted environment
overlay used by provider credential lookup and authorized child-process
surfaces. `src/shell_integration.rs` owns only CA's exact managed zsh block.

Fresh launch authority is resolved before workspace-dependent registries.
`src/worktree_setup.rs` parses native typed setup and bounded Cursor-compatible
metadata, binds the full plan to a SHA-256 digest, and never silently runs
headless setup without that digest. `src/runtime_diagnostics.rs` owns rotating
metadata-only logs and deterministic watchdog state. `src/terminal_awareness.rs`,
`src/mermaid.rs`, Markdown rendering, and TUI overlays own focus/theme/title,
safe links, bounded diagram rendering, message-part copying, and repaint
diagnostics without changing stored conversation bytes.

`src/acp.rs` is a stable ACP v1 stdio adapter over the official Rust SDK. It
reuses CA's local orchestration, provider, session, tool, approval, and cleanup
boundaries and remains byte-independent from `ca.serve.v1`. `src/local_automation.rs`
owns strict pre-init version checks, bounded host Git context, and explicit-path
rule activation. `src/main.rs`, `src/session.rs`, `src/headless_contract.rs`, and
`src/run_environment.rs` provide bounded ephemeral history, collision-checked
caller session IDs, automatic workspace-context exclusion, and opt-in key-only
environment observations. Local `ca repo` commands emit versioned metadata only
and perform no provider, account, or network query.

## HRS-203 delegation, lifecycle capabilities, and skill curation

`src/agent/subagent.rs` remains the authority-owning child manager. It now
validates optional local structured-output contracts, permits exactly one
repair request, atomically admits prevalidated batches, and consolidates them
in submission order. Spawn admission shares one global pause lock across
single, nested, batch, and plugin lifecycle paths. Optional isolation delegates
to `WorktreeStore`; shared workspace remains the default. Terminal snapshots
carry bounded inline output or a private digest-bound spill, detailed timeout
state, and durable attribution for admitted background jobs that survive the
child and remain host-controllable.

`src/agent/lifecycle.rs` is the trusted in-process lifecycle capability layer
exposed through `src/plugin.rs`. Random versioned handles bind child, origin,
correlation, and expiry for launch/status/wait/cancel/result/reconnect.
Correlation admission is serialized, terminal retention is bounded, and
forged, expired, or cross-origin handles fail closed. Declarative plugin
manifests gain no native execution authority from this facade.

`src/app.rs` and `src/main.rs` project detached terminal snapshots into only
the originating conversation as one host-owned system lifecycle message. The
idle event loop may start one continuation; no completion becomes user input
or crosses a conversation/turn boundary. `/agents pause|resume` and the typed
manager tools affect future admission only.

`src/skill_curator.rs` owns an opt-in private curator state and crash-recoverable
prepared/moved/committed journals for archive, restore, consolidate, pin, sync,
and adopt. It mutates only managed native skills, never foreign read-only
sources. `src/learning_journey.rs` extends the local graph projection with
bounded in-scope memory metadata and skill-memory evidence edges; rendered
memory nodes disclose scope, kind, normalized term count, and short digest but
not stored memory text.

## HRS-202 Hermes-informed learning graph and child lifecycle

`src/skills.rs` accepts bounded descriptive `category` and `related_skills`
metadata for portable skills, including the compatible
`metadata.hermes` namespace. The fields are normalized and validated with the
portable skill name but remain non-authorizing: they cannot activate skills,
grant tools, install dependencies, or resolve ambiguous selectors.

`src/learning_journey.rs` derives `/learning graph` from the current skill
registry and the existing digest-bound journey ledger. The bounded view joins
current-definition usage/outcomes, declared relationships, and successful
same-run co-use, while naming unresolved declarations and ignoring stale
digests. It reads no skill bodies and adds no persistence, background
curation, or automatic asset mutation.

`src/agent/subagent.rs` owns child steering receipts and terminal handoff
identity. Accepted steering is ID-bound and retained until the provider runner
delivers it at a request boundary; approval waiting no longer consumes it.
Send acceptance and terminalization share the manager lock, so an accepted
message is either delivered or exposed as bounded sanitized `missed_steer`.
`await_agent` optionally returns a current live snapshot after a bounded wait,
and terminal summaries expose an immutable SHA-256 digest. These additions do
not widen child grants, workspace access, approvals, cancellation ownership,
session storage, or provider routes.

## TUI-189 model catalog availability presentation

`src/model_catalog.rs` owns bounded optional model discovery and reports live
catalogs, documented model sources, and sanitized lookup failures separately.
`src/main.rs::apply_model_catalog_refresh` and
`apply_provider_catalog_refresh` project those results into process-only picker
status without treating discovery freshness as a provider health check.

A provider is live when its catalog returned, fallback when configured or
documented models remain after a failed lookup, and unavailable only when a
failure has no usable model source. The footer derives mutually exclusive
counts before merging discovery results into the existing model list. This
presentation path does not change provider probing, authentication, routing,
configured models, selection persistence, provider calls, or sessions.

BUG-237 gives the exact first-party OpenAI Responses and ChatGPT Codex routes
the same supported GPT-5.6 Sol/Terra/Luna fallback catalog. A failed Codex
catalog lookup therefore no longer leaves only its configured starter model.
Provider names on custom endpoints do not receive these built-ins; live
discovery remains separate and account-specific access is not inferred.

## TUI-188 collapsible edit and command activity

`src/agent/registry.rs` owns the internal presentation class for direct file
mutations and commands. `src/tui/render.rs` groups runs of two or more across
tool-only assistant messages, bounded by visible content, other tool classes,
user messages, and recorded turn IDs. The first call ID owns process-local
expansion state; canonical calls, outputs, provider messages, and sessions are
never rewritten. `src/main.rs` retains exact successful mutation receipts in
`AppState` for live Created/Edited/Deleted presentation, while restored history
uses conservative tool/argument/marker inference.

Collapsed groups are status-aware bounded headers. Expanded groups wrap
workspace-relative underlined file rows and complete sanitized command text,
then reuse the existing diff, semantic-command, and adaptive-output renderers.
`TranscriptScrollState` stores only hit regions rebuilt from the current
wrapped document. Mouse release at the selection anchor toggles that exact
header; movement remains selection, and pager/overlay rendering clears or
isolates transcript targets. Ctrl+O continues to select the latest expandable
block through the same stable key.

## ASY-187 token-free process observation

`crates/ca-watch/` is the reusable process-observation boundary. Its versioned
request and event types validate bounded local or SSH targets; `WatchService`
owns cancellation, snapshots, ordered broadcasts, identity continuity,
degradation recovery, and capped retry timing. `ProbeTransport` separates local
and fixed-SSH inspection from the generic/copy adapters, so new process kinds do
not change CA's event loop or TUI. The remote transport always invokes
`/usr/bin/ssh` with fixed batch, timeout, no-TTY, and no-forwarding arguments and
one fixed probe script; it accepts no caller-supplied command text or key data.

`src/process_watch.rs` adds CA's originating-session ownership around that
service. `watch_process` and `stop_watcher` are Main-profile tools, while
headless, service, scheduled, Plan, and subagent surfaces filter or omit them.
Workspace preparation attaches network capability only to validated SSH watch
requests, so the ordinary interactive approval binds the exact destination,
PID, process kind, paths, and cadence. Stopping or session shutdown cancels only
the observer; observed PIDs remain outside `JobManager` ownership.

The existing Tokio TUI loop consumes owner-filtered `WatchEventV1` broadcasts
as redraw-only state. `AppState` projects the latest event into the activity
strip and unified `/jobs` view, retains terminal watcher history for the process
lifetime, and emits completion/failure toasts. This path never appends model
messages, begins a model turn, calls a provider, or changes token accounting.
Explicit background shell jobs share the activity surface but, unlike
foreground commands, no longer inherit the configured foreground timeout.

## TUI-186 adaptive tool-output layout

`src/tui/output_layout.rs` is the process-only layout boundary for ordinary
tool output. It sanitizes terminal text, segments only stable tab-separated
runs, computes grapheme/display-cell widths, allocates bounded table columns,
and returns styled row kinds plus truthful source-line omission counts. It does
not own command capture or canonical tool results.

`src/tui/render.rs::render_tool_output` selects semantic or raw display text as
before, preserves the special send-message and diff branches, and delegates the
ordinary card body to that layout. Cards consume the available transcript
width, pad every content row, and draw a complete right edge. Collapsed and
Ctrl+O-expanded presentation never mutates `ToolCall.output`, provider/session
history, generated files, or `/output full` streaming.

## REL-185 single interactive command approval

`src/approval.rs` prepares one canonical invocation and lets the calling
surface choose whether requested network capability is a separate gate or part
of the tool decision. `src/main.rs` uses the combined form for ordinary TUI
built-ins, matching the Hermes/MiMo interaction model: Allow, AutoReview, and
RunEverything carry the prepared capability into execution; Ask produces one
tool prompt; Deny blocks. Non-interactive and specialized authority surfaces
retain their existing separate contracts.

`src/tui/render.rs` extracts the complete sanitized command and optional cwd
from prepared `run_command` arguments for wrap-capable approval transcript
lines. `src/tui/input.rs` recognizes slash commands only when the leading token
starts at byte zero, while prose paths remain ordinary composer text. In
`src/agent/tools.rs`, nonterminal job polling is a successful observation;
terminal failure classification remains unchanged.

## TUI-184 native macOS composer editing

`src/tui/event.rs` enables the terminal's disambiguated-keyboard protocol on a
best-effort basis and balances it across startup, suspend/resume, failure, and
drop. Compatible terminals can therefore report Command separately from
Option and ordinary keys; terminals that do not support the protocol continue
with their existing input behavior.

`src/tui/input.rs` owns the native composer gestures. Option+Backspace deletes
to the previous Unicode word boundary, Command+Backspace deletes to the start
of the current hard line, Option+Left/Right moves by word, and
Command+Left/Right moves to the current hard-line bounds. These paths normalize
UTF-8/grapheme cursors, expand deletions across registered atomic composer
elements, retain attachment bookkeeping, and remain available while inline
path completion is open. `src/keymap.rs` reserves Option+Backspace for editing;
Remove Last Attachment now defaults to Ctrl+X A.

## LRN-176 foreign knowledge interoperability

`src/foreign_knowledge.rs` owns read-only interoperability with reusable local
knowledge produced by Codex, Hermes, and MiMo. It declares deterministic
workspace/user skill and prompt-command roots, projects bounded sanitized
memory documents into an ephemeral query index, and resolves matching MiMo
project memory through query-only SQLite. Foreign text is always labelled as
stale untrusted evidence and never becomes instruction authority.

`src/main.rs::discover_skill_registry` mounts compatible foreign skill trees as
`Managed` instances, so normal structural validation, exact selectors,
activation budgets, runtime approvals, and ambiguity rules still apply while
`skill_manage` remains unable to mutate their source. `src/prompt_commands.rs`
discovers MiMo/OpenCode/Codex-compatible prompt commands as inert ordinary
prompt expansions; CA built-ins and CA-owned workspace commands retain
precedence.

`src/memory.rs::MemoryRecall` merges query-ranked foreign notes with native CA
memory without persisting their text. `src/codex_learning.rs` additionally
registers Hermes's `~/.hermes/state.db` beside native CA, Codex, and MiMo
history. The Hermes adapter opens SQLite read-only/query-only and projects only
completed main-session requests, final conclusions, safe tool-input shapes,
status/error classes, and recoveries. It never selects reasoning columns,
never exposes tool output, and excludes an entire session containing email
activity. Manual `/learn` and `/distill` also receive the bounded foreign-memory
projection and inventory foreign assets before publication.

## LRN-171 direct adaptive learning

LRN-171 supersedes the manual TUI approval presentation while retaining the
private archive and journal boundaries. A direct run auto-discovers native,
Codex, MiMo, and memory sources, builds an ephemeral 30-day index, and exposes
only bounded search, selected-session inspection, asset inventory refresh, and
strict final submission. Five investigation turns, one forced-final turn, and
two repair turns form the hard cap. `/learn` may publish memory and assets;
`/distill` may publish assets only.

`src/prompt_commands.rs` owns workspace/profile command discovery, literal
placeholder expansion, built-in collision refusal, exact generated replacement,
and generated command/agent publication. The TUI refreshes command and active
profile-agent registries after publication and undo.

Direct runs bind publication authority to the active TUI workspace instead of
the historically dominant corpus. A profile asset requires cross-workspace or
explicit global authority and is rejected if it contains repository-specific
paths. Memory and every generated asset are preplanned under one
prepared/applied/committed journal; synchronous failure or explicit Escape
rolls the whole group back, startup recovers incomplete phases, and one receipt
undo reverses every member workspace. The resumable adaptive checkpoint stores
only completed logical-turn counts and selected session identifiers.

## Repository Layout

`/Users/aji/project/ca` is a four-package Cargo workspace with one shared
`Cargo.lock` and one root `target/` directory:

- `.` builds the `ca` terminal agent and owns provider, permission, role-run,
  headless, service, and operator-cockpit boundaries.
- `crates/ca-watch` builds the reusable typed process/copy observation library
  and CLI; stopping observation never signals the observed process.
- `crates/codex-team` builds the `codex-team` scheduler/governor and owns task
  admission, orchestration, evidence, acceptance, and commit coordination.
- `crates/codex-team-protocol` owns only the versioned read/observe/verify wire
  types, limits, structural validation, and generated schemas. It contains no
  repository access, process runner, learning, or orchestration authority.

The authority split is deliberate. `codex-team` may invoke the strict public
`ca role-run` boundary, but it does not take ownership of `ca` profiles,
sessions, credentials, native task graphs, or provider internals. `ca` does
not become a second scheduler during an orchestrated cycle.

INT-190 makes that boundary bidirectional without creating a recursive
scheduler. Before a Worker, `codex-team` may satisfy typed mechanical work in
Rust and pass bounded query context to exactly one MCP-free `ca role-run`.
Conversely, an opted-in ordinary interactive or headless CA profile advertises
one `delegate_mechanistic` facade on the first model turn. CA supplies the
current repository revision, then directly calls and polls an exact local,
network-disabled, zero-resource, delegate-tier codex-team MCP server. Raw tools
from that server remain hidden from model MCP search. Plan, review, checkpoint,
scheduled, service, subagent, bare, and `role-run` surfaces omit the facade.
`/delegate` and `/jobs` operate on process-local metadata and durable job IDs;
refresh, cancellation, and fixed feedback make no provider request and do not
alter token accounting.

INT-193 makes this cooperation host-automatic without widening the facade.
Before the first ordinary Main/headless provider request, CA's deterministic
intent parser may ask codex-team for at most four bounded composite repository
queries and attach a 16 KiB redacted receipt as request-only context. After a
resolved mutating tool batch, exact mutation receipts select focused typed
verification; an unscoped mutation is first checked by pinned repository
revision and runs full ecosystem verification only when policy permits it.
Provider continuation waits for the durable result. A real failure permits at
most two evidence-driven repair continuations and cannot be reported as
success. Successful isolated evidence is cached only against revision, typed
recipe, affected-input bytes, toolchain/environment fingerprint, working
directory, and isolation backend. Process-local typed events deduplicate turn,
edit, drift, delegation, watcher, and due-schedule observations without
creating provider traffic. Schedule V2 may carry a pure mechanistic query or
evidence target that makes no provider request and creates no session.

INT-194 makes the verification choice dependency-aware and the failure path
recoverable. Codex-team builds a bounded process-local graph from isolated
offline Rust/Swift metadata plus tracked Node/Python manifests, then maps exact
mutation paths through owners and transitive reverse dependencies to typed
package/target recipes. Graph failure, fan-out, cross-cutting inputs, or mixed
ecosystems may escalate only when CA's typed request carries active-profile
authority for a full suite. Failed durable evidence is classified without a
rerun before a repair pass. CA records atomic multi-file receipts, presents a
process-local `/evidence` ledger, persists only hash/category quality history,
and may resume an exact revision/digest/budget-bound verification checkpoint
when a saved session is explicitly opened. A terminal verification event can
authorize at most one provider continuation; polling, feedback, cancellation,
watchers, and scheduled completion remain model-free.

Roadmap ownership is separate: INT-190 owns this typed mechanistic cooperation
boundary, while INT-151 continues to own learned provider/role routing and may
not widen the facade's read/observe/verify ceiling.

SEC-154/SEC-155 keep privilege handoff outside generic command and job
authority. `src/privilege.rs` owns the closed `run_privileged` request, fixed
per-platform sudo capability probe (`/usr/bin/sudo` on macOS/Linux and
`/usr/local/bin/sudo` on FreeBSD), secure native-executable and ancestry
validation (Mach-O on macOS, ELF on Linux/FreeBSD), versioned
invocation digest, one-use nonce, detached private process groups, and
streaming byte redaction. The digest binds canonical program identity, ordered
argv, workspace cwd/roots, sandbox mode, and helper identity; both identities
and bound directories are revalidated immediately before execution. The
process-only handle additionally binds the invocation nonce to the initiating
interactive job owner.

Only the main interactive TUI advertises the tool when the platform's fixed
sudo passes the root/setuid/ancestry/native-format probe. `src/main.rs` gives
each exact invocation its
own `[y]`/`[n]` manual resolution, runs credential-free pre-tool hooks, checks
cached authorization, and opens `src/interaction.rs`'s bounded zeroizing masked
input only for a cache miss. Authentication and target execution are separate:
the password validates through a detached helper process, while the prepared
sandbox target runs through `sudo -n` with stdin closed and network denied.
Privilege processes are never registered with `JobManager`, so `/jobs` and all
model-facing job controls have no handle to discover or guess. Headless,
service, scheduled, Plan-mode, subagent, unsupported-OS, and failed-probe paths
omit or reject the capability. Because the mandatory sandbox backend remains
macOS-only, Linux and FreeBSD reject `required` before approval or credential
entry; their support is intentionally limited to an explicitly selected
`policy-only` or `off` mode and never silently downgrades sandbox policy.

OPT-152 treats hosted-model efficiency as an explicit prompt, result, and
measurement boundary. The deterministic session-epoch prefix in `src/main.rs`
encourages symbol-first narrow retrieval and one-turn batching of independent
read-only work. `src/context.rs` compacts canonical working state into separate
facts and hypotheses plus preserved open uncertainty. Neither path introduces
request-volatile cache-prefix material.

`src/command_output.rs` remains the sole conservative Git/Cargo semantic
classifier. Under the explicit semantic policy, interactive, headless, and
service orchestration store that bounded summary as the canonical model tool
result; default bounded policy and ineligible commands retain their original
model bytes. Human-facing events retain raw results, while the interactive
process keeps bounded raw capture for expansion. `src/request.rs` exposes only
the declared stable-prefix boundary/estimate. `src/usage.rs` derives new input
and cache ratio only from complete cache-read detail, counts provider tool
continuations rather than local executions, and labels total and
first-observed-output latency separately.

PRV-151 separates provider prompt-cache policy from every other cache-shaped
state. `src/provider_contract.rs` resolves an exact endpoint/protocol/model
request and reporting contract before credentials, with exact custom
declarations from `src/config.rs` only when no built-in contract owns those
formats. Provider-managed routes emit no controls; explicit OpenAI Responses
routes derive their private key from `PromptCacheScope`, `RouteCacheIdentity`,
the stable session-epoch digest, and canonical tool digest.

`PromptAssembly` in `src/request.rs` types provider messages by lifetime:
session epoch, append-only conversation, frozen logical turn, and request-only
material. Interactive orchestration freezes the logical-turn envelope and tool
catalog through continuations, while branches, side questions, subagents, and
maintenance work receive separate scopes/domains. MCP declarations retain
built-in order and sort remote tools by stable identity; recursive schema
canonicalization is used only for equality and hashing.

`src/context.rs` projects a validated checkpoint as one session-epoch message
plus uncovered canonical history. Versioned source digests fail closed to full
history for tampered/future data, and completed checkpoints remain pending until
the current logical turn finishes. Checkpoint generation separately caps provider
output at 16384 tokens (including reasoning), while the saved summary remains
limited to 4096 estimated tokens/32 KiB. Explicit Length/max_output_tokens
termination permits one same-prompt retry at 32768 tokens, shared across the
single quality repair. A one-attempt transport policy bounds the entire operation
to three physical requests with separate usage spans. Partial/filter/unknown
responses cannot publish a checkpoint; cancellation still owns the whole worker.

An unadvertised interactive tool call resolves to an Error with a sorted bounded
JSON list of exact advertised names, conditional search_tools guidance, or an
explicit plain-text instruction for empty catalogs. Legacy and generated Error
markers share the existing single-correction allowance within the active turn.
Repeated mismatches preserve restrictions and show mode guidance first so it
survives narrow footer truncation. Frozen /retry keeps its original schemas;
switching mode applies to a fresh user request. Provider adapters normalize their native
usage dialect into provenance-bearing read/write/miss counts; `src/usage.rs`
and `src/pricing.rs` enforce subset invariants and explicit replacement,
surcharge, or included write billing before compatibility projection. The TUI
edits exact model declarations through `/cache provider/model`; legacy
provider-wide policy remains read-only configuration fallback.

LRN-166 keeps archive acquisition, semantic processing, and knowledge mutation
as three separate authorities coordinated by one stable service.
`src/codex_learning.rs` streams the default profile's native active/archive
sessions plus Codex active/archive JSONL roots into bounded Dream/Distill
corpora. Its persistent archive index contains only source revisions, session
and workspace relationships, message identities/digests, counts, and review
state. LRN-150's manual source/review/proposal commands remain compatibility
surfaces, but the old indexing-only LaunchAgent is migrated away.
LRN-168 layers a process-local typed TUI workflow over those unchanged store
operations: registered-source sync, pricing, provider generation, and proposal
application run on a dedicated generation-tagged task; exact digests stay in
application state; evidence and proposal decisions remain separate and
Later-first. A pending inert proposal is shown before any new sync. Drift causes
one refresh and always clears approval; repeated drift stops without provider
contact and requires an explicit refresh.
LRN-169 makes that generation observable and interruption-safe. `/learn
approve` resolves only the currently priced revision, reports deterministic
Dream/Distill call boundaries from `0/N` through `N/N`, and retains progress
while its pager is closed. A separate strict owner-only `progress.json`
checkpoints validated results after each call and binds them to the exact
evidence digest, provider/model, price, and materialized call-plan digest.
Resume executes only missing calls; any binding drift refuses reuse, and final
proposal publication removes the checkpoint.
LRN-170 collapses reviewed Dream and Distill analysis into one strict structured
request per compacted corpus. The request inventories enabled portable skills
and custom agents, and returns separately validated memory operations, inert
skill packages, and a bounded decision shortlist. A combined checkpoint is
written only after source authority, exact Dream-owned update targets,
secret/text bounds, auto-distilled provenance, and three-session skill evidence
validate. Memory-only reduction corpora cannot emit skills. Publication still
uses the existing independent Dream/Distill journals and undo records.
The first `session_meta` owns identity; only user text and assistant
`final_answer` text committed by `task_complete` are candidates. Raw transcript
text exists only during a scan or review render and never enters the index or
ordinary session history.

Manual `ca learn run` also discovers
`~/.local/share/mimocode/mimocode.db` when present. It opens one query-only
SQLite snapshot, excludes sessions recorded as foreign imports, reads only the
empty-or-`main` main-agent slice, and admits only user text plus a final
assistant answer with `time.completed` and no error. Between that user and
final answer, safe bounded tool names and selected inputs (commands, paths,
queries, patterns, tasks, and targets) are projected as workflow evidence.
Outputs, content bodies, email activity, reasoning/file text, synthetic or
ignored text, incomplete/error turns, subagents, and secret-shaped inputs are
excluded. MiMo session directories retain workspace partitioning and
parent-session links retain fork deduplication, preferring the richer
workflow-bearing copy. Timestamped sessions are compacted recent-first while
older corroboration remains eligible. The index stores only bounded identities,
content/revision digests, counts, and relationships; MiMo text is materialized
transiently and never enters `/sessions`.

Autonomous harvest queues enrollment-bound immutable intents without deleting
source evidence. The `codex-team` supervisor freezes intent/evaluation state,
runs baseline and candidate replays plus independent judgment, and delivers one
sorted-canonical-JSON completion request. Only a terminal rejection or exact
replay-approved Memory/SKILL.md artifact is legal. `ca` publishes through the
existing Dream/Distill journals, conflict checks, generation limits, and undo
path, durably records an idempotent receipt, then acknowledges archive evidence
and removes the queue record. Provider-visible corpora contain workspace
digests and synthetic evidence IDs, never source paths.

The macOS `com.codex-team.self-improvement` LaunchAgent invokes one private,
digest-bound supervisor at login and keeps it alive. Each tick asks the enrolled
`ca` binary for a locally bounded harvest, runs at most one workflow worker,
and defers provider work while a foreground lease exists. Knowledge-only
enrollment binds exact component binaries, the four native/Codex roots plus
the MiMoCode database when present, default
publication profile, named execution routes, and unbounded metered spending.
The generated execution profile contains provider/model metadata and external
credential references only, forces `approval_mode = "never"` and required
sandboxing, and omits hooks/MCP/arbitrary permission configuration. Health,
route, vault, or disk failure safety-pauses the service. No enrollment path can
activate source, binary, routing, permission, provider-setting, role-prompt,
executable, plugin, dependency, or ordinary-session mutations.

REL-149 keeps tool exposure, consent, and execution tied to one prepared
invocation. `ToolSelectionConstraints` derives skill activation selectors,
skill resource instance IDs, and LSP availability from the effective runtime
registries/managers. Unknown, disabled, or ambiguous skill selectors fail
before approval. Post-mutation LSP handling is typed as `NotApplicable` or a
bounded `Report`, so no configured/matching server produces no transcript
suffix while real diagnostics and failures remain visible through the existing
status bookkeeping.

File mutation remains file-scoped. `apply_patch { path, patch }` parses bounded
unified hunks, treats optional file headers as non-authoritative, validates the
complete old image and hunk counts/order, and relocates only one unique exact
match. It rejects file lifecycle/binary/mode operations and writes only after
all hunks succeed. Newline style and EOF-newline state are preserved; only a
successful write creates an undo receipt or post-edit diagnostic request.
Repeated identical mutation failures enter the process-local loop guard.

Interactive provider batches store immutable prepared arguments in ordered
manual resolutions. Immediately before each approval is shown, the shared
permission classifier re-evaluates live persistent rules, exact current-run
grants, and approval mode. Exact path grants use escaped canonical paths;
pathless and command grants use normalized invocation identity, including
canonical cwd, background, arguments, and network request. For ordinary TUI
built-ins, the prepared network capability travels with the same tool
decision; explicit Ask and Deny remain ceilings, while one-shot allow/deny
never changes later classification. Non-interactive surfaces keep their
separate network authority contracts.

Ordinary command jobs still use inherited environment and non-login
`/bin/sh -c`, but sandbox preparation now adds narrow read-only executable and
Cargo registry roots for the approved command. AutoReview and sandbox setup
share a canonical trusted-executable resolver; arbitrary inherited `PATH`
directories cannot gain silent authority. Literal Cargo stages run offline
unless effective network authority was separately prepared and granted.
Process-capable `read_lints` uses the managed job path and is absent from
automatic read-only child grants.

The ordinary TUI surface uses progressive disclosure at the render boundary.
An empty idle transcript contributes no welcome chrome; the title is the single
context row, the empty composer is borderless, and the status row advertises
`/ commands`. Active turns, queues, scrolling, approvals, failures, and overlays
retain their contextual controls and complete content. Title fields already
configured in the status line are omitted from the title so each fact appears
once. Slash-command identity and filtered matching remain unchanged; only an
empty query receives a stable common-action-first presentation order.

Legacy approval compatibility is resolved at the configuration boundary.
`ConfigStore` treats only the exact saved `allowlist` value as a migration
marker, exposes AutoReview to the current process, and atomically persists
`auto-review` only after the complete configuration has validated. Canonical
`ask` remains strict. The `/auto-review` command uses the same atomic patch
boundary to save either `auto-review` or `ask`; bare profiles remain
process-only. Tool execution continues through explicit policy and network
checks before the registry/closed-shell read-only classifier, so this migration
does not widen workspace, mutation, network, or unknown-command authority.

Direct provider credentials use a profile-scoped private-file primary on every
supported host. On macOS, `MigratingProviderSecretStore` consults the legacy
`ca.provider-api-key` Keychain adapter only when the primary credential is
absent, and it must atomically commit the imported bytes before returning them.
Later reads, saves, and deletes touch only the primary; the legacy item remains
available for rollback. Default credentials live below
`~/.local/share/ca/credentials/`, named-profile credentials below that
profile's data directory, and bare credentials remain memory-only. Codex OAuth
and the separately scoped email-gateway Keychain do not cross this boundary.

Interactive exit presentation crosses the terminal ownership boundary as an
optional value: `run_tui` derives a sanitized bounded session handoff, returns
it after owned workers shut down, and `run_standard` prints it only after the
TUI value has dropped and restored raw mode and the alternate screen. The
handoff reuses the canonical session title and UUID-backed public identifier;
empty and persistence-disabled runs return no value. Headless and service
protocols do not pass through this presentation path.

AutoReview's `run_command` exception is a bounded, stage-aware local-read
classifier rather than a blanket shell grant. The closed parser rejects shell
composition that can widen execution, while `src/permission.rs` applies
utility-specific mode checks. In particular, `find` discovery and stdout-only
`sort` may run without approval; execution, deletion, output-file, and helper-
program modes remain gated. Explicit policy and network decisions still
precede this convenience classification.

AutoReview also classifies asynchronous `spawn_agent` arguments dynamically.
Only a nonempty set of registered read-only subagent tools without network,
external-communication, or nested-agent capability, and with no command grant,
can skip the prompt. Persistent Deny/Ask decisions retain precedence;
synchronous `task`, unknown tools, mutation, shell, network, and strict
Allowlist behavior remain unchanged.

Agent route tables contain provider specifications and shared request sinks,
not prebuilt credential-bearing clients. A child resolves its specification
asynchronously at start and then constructs a transient client. Codex routes
pass through the canonical runtime-provider resolver under a serialized gate
so cached ChatGPT OAuth can load or refresh before traffic; other providers
bypass that resolver. Resolution failure precedes request capture and usage
recording. Omitted complexity selects the stored parent route, while explicit
complexity remains a fail-closed inter-provider cost-routing request.

Identical contiguous completed `spawn_agent` failures are grouped only in the
transcript projection. The first call ID owns the expandable card; canonical
messages and every call's ID, status, output, hooks, session representation,
and provider result remain untouched.

Project scratch storage is versioned, profile-isolated, and keyed by the
canonical primary workspace beneath the profile data directory. `/scratch` is
the only mutation surface; `read_scratch` returns a sanitized filtered 16-KiB
view. Scratch content is not serialized into sessions/exports or injected into
provider prompts automatically.

Command-output policy is stored under `agent.command_output`. Eligible Git/Cargo
summaries live only in process state; canonical tool output, provider history,
sessions, and exports retain the existing bounded raw result. `/output full`
uses the ordinary job sandbox and native terminal suspend/restore boundary.

Reasoning phases are timed process-locally from the first reasoning fragment to
the first text, tool call, or completion boundary. Sessions persist only an
optional turn/message annotation with the frozen duration. Conversation-origin
child agents publish bounded sanitized watch snapshots for inline rendering;
only typed terminal summaries are durable. `/context` composes exact context
estimates with current-sitting usage/speed, workspace roots, non-activating LSP
status, and bounded task status without creating a drawer or starting work.

Low-yield detection consumes bounded typed tool outcomes before transcript
presentation. `LoopGuard` keeps only process-local normalized edit and failed-
command observations, then gates the post-batch provider boundary. Interactive
turns pause for an explicit Replan, Continue, or Cancel decision; service mode
emits a structured interaction, while unattended runners fail with the typed
`low_yield_loop` outcome. No detector evidence or suppression state is durable.

Composer attachments use a generation-tagged background index over every
declared workspace root. `src/attachments.rs` owns ignore-aware discovery,
fuzzy/frecency ranking, typed file/MCP references, strict line ranges, and
send-time no-follow validation. The TUI consumes bounded snapshots and never
waits on traversal or MCP locks. Picker-visible MCP rows come only from already
running catalogs; selected bodies are resolved under existing gates into a
bounded transient request message and never enter canonical sessions.

Transcript metadata is an optional presentation layer over canonical messages.
Per-message annotations may carry the actual provider/model plus total turn
duration and typed terminal outcome; old records default every value to absent.
The event-loop completion boundary updates only the final annotation for a
turn. Default-off timestamps and assistant footers read those stored values,
while optional status fields consume the same current-sitting summary used by
`/context`, preserving incremental, subscription-allocation, mixed, and
unknown cost rather than estimating missing data.

Prompt drafts are persistent only through explicit stash actions. The bounded
profile-owned store contains inert UTF-8 composer text and timestamps, never
image payloads or validated attachment authority. Restore removes the selected
entry and, when necessary, atomically swaps the current non-empty composer into
the same store. Bare mode refuses persistence. Restored `@` references return
to ordinary composer text and pass through the normal send-time attachment
validation before any content is read.

Dream and Distill are bounded profile maintenance, not general autonomous
execution. `src/learning.rs` owns due-state, sanitized corpus construction,
strict one-request report schemas, the Dream memory journal, auto-distilled
skill generations, and exact-digest undo. Only an idle interactive persistent
profile may schedule them; user input cancels first, excluded runners cannot
enable them, and automatic routing accepts only an eligible priced `standard`
route. Dream can mutate only Dream-owned records, while Distill can publish or
replace only one-file `auto_distill` generations under the profile skill root.

## System Boundary

`ca` is one Rust binary. It owns terminal interaction, conversation state, provider requests, local tool execution, approvals, sessions, and scripted tests. It does not compile or import anything under `oracles/` or `reference/`.

## Module Map

| Path | Responsibility | Change here when |
|---|---|---|
| `src/main.rs` | CLI parsing, mode selection, tokio orchestration, streaming events, tool-loop continuation | changing request lifecycle, event-loop coordination, headless behavior, or CLI flags |
| `src/role_run.rs` | strict public orchestrator-role preflight/request validation, pinned workspace authority, fixed tools/grants, restricted route preparation, and role subprocess supervision | changing `ca.role_run.v1`, `ca.role_run_preflight.v1`, role-only capability/permission boundaries, or role cancellation |
| `src/bin/reasoning_artifact_experiment.rs` | compile-time-gated `reasoning_artifact.v1` tools plus the RND-097 preregister/fixture-check/price/run/resume/report controller | changing the isolated experiment CLI; ordinary `ca` remains separate |
| `src/reasoning_experiment_protocol.rs` | feature-wired RND-097 controller/process protocol with strict one-call provider/tool specs and bounded framed IPC | preserve digest/nonce binding, frozen route/options, redacted Debug, and distinct tool-versus-provider authority; phase-level worker types are controller identity only |
| `src/reasoning_experiment_ledger.rs` | feature-gated exact atto-USD, persisted frozen price card, and owner-only immutable spend events | bind every physical provider POST to one durable reserve/terminal transition; never use it as tool or credential transport |
| `src/reasoning_experiment_worker.rs` | feature-only hidden one-POST OpenAI broker and separate clean local-tool worker | preserve framed one-call authority, pre-POST reservation, transient bodies, scrubbed tool environment, and the no-credential tool boundary |
| `src/bin/reasoning_artifact_experiment/fixtures.rs` | RND-097 six-fixture/hidden-verifier seam with WSP-096 materialization | preserve hidden/golden isolation, external Cargo targets, manifest/recovery binding, and explicit cleanup |
| `src/bin/reasoning_artifact_experiment/{pilot,trial_state,trial_runner,controller,arm,conversation,recovery}.rs` | immutable pilot identity/state, six-arm orchestration, provider/tool turns, verification, recovery proof, safe resume, and reporting | preserve legal phase transitions, planner/executor isolation, canonical-route gates, body-free durability, and no ambiguous replay |
| `src/app.rs` | canonical UI/conversation state and shared enums | adding state that multiple TUI/orchestration modules own |
| `src/agent/client.rs` | protocol-neutral provider HTTP lifecycle, bounded retries/deadlines, runtime authority, canonical message/tool types | changing shared provider transport policy, request dispatch, or compatibility message construction |
| `src/provider_trace.rs` | explicit default-off profile-scoped provider-exchange traces, bounded asynchronous JSONL storage, redaction, inspection, and exact deletion | changing `/trace`, `--trace`, provider wire observability, trace retention/bounds, or trace storage safety |
| `src/project_init.rs` | bounded primary-workspace evidence, deterministic `AGENTS.md` draft validation, fingerprint revalidation, and no-follow create-new publication | changing `/init` evidence, draft bounds, target safety, or undo receipt construction |
| `src/security_audit.rs` | deterministic bounded no-follow audit of loaded instruction, skill, hook, MCP, permission, credential-reference, and executable surfaces with sanitized typed findings | changing `/audit`, `ca audit`, audit authorities/rules/bounds, or audit exit semantics |
| `src/raw_transcript.rs` | deterministic privacy-filtered projection of canonical visible conversation data plus exact native-scrollback presentation and Enter acknowledgement | changing `/raw`, transcript omission/redaction rules, or native output bytes |
| `src/agent/provider.rs` | defaulted provider-protocol identity plus adapter-owned Chat Completions, OpenAI Responses, and Anthropic Messages path/auth/byte serialization/SSE decoding/usage mapping | adding or changing a complete provider wire adapter |
| `src/provider_contract.rs` | exact official endpoint/protocol/model resolution for built-in OpenAI/Codex, MiMo, and DeepSeek capabilities and wire dialects | changing first-party model capabilities, reasoning mappings, endpoint identity, or proxy isolation |
| `src/provider_contract_verifier.rs` | explicit family-filtered two-request/no-retry synthetic-tool verification with runtime OAuth resolution and sanitized versioned reports | changing `--provider-contract`, live provider-contract checks, request caps, or report safety |
| `src/image_content.rs` | bounded normalized process-only image payloads, typed text/image content blocks, safe durable markers, and redacted diagnostics | changing multimodal request content, image limits, or persistence-safe metadata |
| `src/browser.rs` | explicitly selected owner-bound local Chromium lifecycle, disposable-profile launch, literal-loopback bounded CDP, network windows, DOM/screenshot/download caps, and process-group cleanup | changing browser actions, executable discovery, CDP limits, network blocking, file results, or cleanup |
| `src/voice.rs` | explicit macOS capture, bounded process-only mono WAV ownership, sandboxed local or exact-provider transcription, typed safe failures, and process-only voice state | changing `/voice`, audio capture/limits, transcription backends, cancellation, or draft delivery |
| `src/external_runtime.rs` | profile-scoped exact-source/Ed25519 trust, closed multi-file runtime catalogs, streamed size/hash verification, private staging, retained rollback, safe metadata, and reverified entrypoint resolution | changing `ca runtime`, managed-runtime trust, downloads, disk lifecycle, or future runtime consumers |
| `src/local_models.rs` | profile-scoped validated GGUF storage plus exact managed llama-server resolution, fixed loopback launch, bounded health/model/log supervision, process-only provider construction, and process-group cleanup | changing `ca local`, managed model bytes, local-server launch/readiness, or explicit local-run integration |
| `src/memory.rs` | profile/workspace-scoped memory records, local validation, process-only proposals, bounded recall indexes, Dream provenance/CAS, and atomic regular-file storage | changing memory scope, proposal approval, Dream ownership, recall injection, export, or persistence safety |
| `src/learning.rs` | bounded Dream/Distill scheduling state, native active/archive corpus construction, supervised export/completion protocol, crash-safe memory/skill journals, status, and exact undo | changing automatic-learning eligibility, publication authority, bridge digests, persistence, or rollback |
| `src/codex_learning.rs` | streamed Codex JSONL and read-only MiMoCode SQLite evidence index, exact typed two-stage TUI/CLI review, manual native/Codex/MiMo harvest, unified `ca learn` status/control, isolated-profile planning, and single-service activation/migration | changing archive bounds/schema observations, evidence/proposal acknowledgement, drift handling, recurring service authority, or activation confirmation |
| `src/goal.rs` | durable finite goal state, limits, bounded evidence snapshots, typed evaluator reports, pause/terminal transitions, and autonomous continuation instructions | changing `/goal`, evaluator authority, goal budgets, or continuation policy |
| `src/workflow.rs` | bounded declarative TOML/JSON workflow definitions, validation, deterministic DAG scheduling, run-local grants, evidence gates, outputs, retries, and durable execution state | changing `/workflow`, workflow authority, scheduling, persistence, or evidence policy |
| `src/agent_graph.rs` | bounded dependency-aware plan-agent definitions, graph validation, ready/blocked state derivation, attempt/evidence policy, and safe summaries | changing `/tasks` graph semantics, dependencies, child grants, attempts, or evidence gates |
| `src/multi_run.rs` | process-only bounded candidate definitions, exact route/cost previews, safe progress/evidence summaries, and validated judgment/fusion reports | changing `/multi-run`, candidate comparison state, evidence staleness, or structured best-of-N results |
| `src/request.rs` | typed prepared-model-request boundary plus bounded, redacted, process-only semantic snapshots and `/request` text construction | changing runner request identity, inspector attribution, redaction, or snapshot bounds |
| `src/agent/registry.rs` | typed tool specs, generated schemas, validators, profiles, origin/mutation/capability/resource metadata | adding/changing tool identity, schema, availability, or policy metadata |
| `src/agent/tools.rs` | validated built-in handler execution, ranged UTF-8/PDF reads, bounded web responses, and output compatibility | changing a built-in side effect or model-visible result text |
| `src/headless.rs` | injectable headless runner, versioned events, output/approval mapping, restricted child-manager dispatch, and exhaustive child shutdown | changing automation output, headless policy, subagent restrictions, or machine-readable lifecycle |
| `src/headless_contract.rs` | opt-in `ca.headless.v2` verification/structured-output events, bounded local JSON Schema contracts, and typed verification reports | changing `--check`, explicit check repair, output-schema validation, or v2 lifecycle fields |
| `src/service.rs` | strict bounded `ca.serve.v1` stdio/WebSocket transport, external token authentication, loopback attach bridge, one process-only session/turn, manual approval/interaction resolution, and owned cleanup | changing operations/events, listener/attach/auth lifecycle, service authority, or service tool/provider orchestration |
| `src/acp.rs` | stable ACP v1 newline JSON-RPC stdio adapter over the official SDK, local session/mode/turn translation, streamed updates, client permission requests, and disconnect cleanup | changing ACP capabilities/methods, permission closure, location normalization, transport bounds, or separation from `ca.serve.v1` |
| `src/json_compat.rs` | recursively sorted JSON compatibility serializer for deterministic pre-ACP byte contracts under the SDK's unified `serde_json/preserve_order` feature | changing persisted or snapshot-sensitive JSON ordering at named compatibility boundaries |
| `src/usage.rs` | process-only provider-request ledger, exact-decimal optional pricing, typed outcomes, first-output-to-finish generation duration, shared current-sitting speed/cost presentation, and local/provider-account `/usage` report composition | changing request accounting, usage subsets, request categories, generation-speed inputs, configured cost states, or report scope separation |
| `src/provider_usage.rs` | explicit-use, process-cached official-host OpenAI/DeepSeek account adapters plus local-only MiMo plan estimation | changing remote quota endpoints, credential/host policy, response bounds, cache behavior, or account-scope presentation |
| `src/pricing.rs` | source-bound official cards, exact OpenAI/Token Plan arithmetic, freshness cache/CLI, capability ceilings, and fail-closed delegation selection | changing price authority, billing eligibility, route scoring, or `ca pricing` |
| `src/mimo_auth.rs` | bounded Xiaomi browser/manual authorization, strict credential-plus-endpoint billing classification, and loopback callback validation | changing MiMo authorization, Token Plan/PAYG classification, secret handling, or callback protocol |
| `src/provider_connect.rs` | transactional first-class provider probes, credential rollback, and billing-only saved Token Plan repair without default-route selection | changing provider connection persistence, probing, billing repair, or selection semantics |
| `src/hooks.rs` | process-scoped asynchronous lifecycle-hook runner, bounded/sandboxed process execution, safe management snapshots, and process-group cleanup | changing interactive main-session hook events, fail-open/closed handling, hook testing, or cleanup guarantees |
| `src/keymap.rs` | typed one- and two-stroke global composer bindings, defaults, parsing, conflict/reserved-key validation, and resolution; navigation is reserved except as a chord's second stroke | changing configurable shortcuts or their safety boundary |
| `src/interaction.rs` | typed interaction requests/state/results, ordered manual resolution with prepared combined tool/network authority, host secret handles | changing questions, choices, modal input, mixed approval/execution ordering, deferred network authority, or masked host prompts |
| `src/activity.rs` | authoritative activity phase, terminal turn outcome, and phase timing | changing busy state, lifecycle rendering, cancellation/failure classification, or queue drain eligibility |
| `src/loop_guard.rs` | bounded process-only typed tool-outcome normalization, similarity/progress detection, incidents, and recovery decisions | changing low-yield thresholds, evidence sanitization, reset rules, or recovery semantics |
| `src/attachments.rs` | cancellable ignore-aware multi-root indexing, nucleo ranking, profile-relative frecency, typed line-range/MCP references, and send-time attachment validation | changing `@` picker discovery/ranking, file reference grammar, frecency bounds, or process-only MCP resource resolution |
| `src/prompt_stash.rs` | explicit bounded profile-scoped composer draft persistence, corruption quarantine, atomic restore/swap, and owner/symlink checks | changing `/stash`, stash caps, profile storage, restore/delete semantics, or corruption recovery |
| `src/model_preferences.rs` | bounded profile-scoped model recency/favorites, owner-only atomic persistence, and unique available-row projection | changing model preference storage, Favorites/Recent sections, or scoped model cycling |
| `src/foreign_import.rs` | bounded closed Codex/Claude JSONL and immutable read-only OpenCode SQLite adapters, visible-text filtering, and path-free provenance construction | changing foreign source formats, traversal/input limits, inert field policy, or import preview data |
| `src/terminal_text.rs` | centralized escape/control sanitization for terminal-facing external text | changing provider/tool/job text safety or supported control-sequence handling |
| `src/ui_settings.rs` | validated built-in themes, default-off transcript presentation settings, ordered status fields, and the bounded network-denied custom-status worker | changing transcript timestamps/footers, `/theme`, `/statusline`, display persistence, or custom status execution |
| `src/terminal_sessions.rs` | bounded exact-argv tmux list/create/attach/detach/kill adapter for user-owned durable terminals | changing tmux ownership, target validation, timeouts, or external-session lifecycle |
| `src/workspace.rs` | stable workspace roots, canonical containment, prepared registry resources, obvious SSH-family network inference, and session-local cwd/root management | changing filesystem/network authority, path preparation, additional roots, or root persistence |
| `src/worktree.rs` | profile-owned Git worktree manifest, derived paths/branches, bounded identity/status/diff inspection, exact attachment contexts, and clean-only branch-preserving cleanup | changing `/worktree`, isolated Git workspace ownership, attachment validation, or cleanup guarantees |
| `src/workspace_materialization.rs` | process-local disposable-workspace admission, descriptor-relative source/destination traversal, manifest verification, APFS regular-file cloning/copy fallback, durable ownership receipts, recovery-artifact binding, and explicit cleanup | changing benchmark/scheduled workspace copying, tracked-path policy, clone/copy accounting, crash reopen, or cleanup authority |
| `src/sandbox.rs` | typed sandbox modes, backend readiness, capability-checked process isolation, filesystem/network profile generation, and least-authority ordinary macOS identity lookup | changing OS process confinement, readiness reporting, Directory Services authority, or explicit policy-only/off behavior |
| `src/skills.rs` | portable Agent Skills and legacy filesystem discovery, immutable qualified instances, bounded metadata/resource inventories, complete digests, task-local loads, safe viewing, ranking, activation, and pin validation | changing roots, frontmatter, inventory bounds, qualification, loading, prompt inclusion, or persistence references |
| `src/skill_cli.rs` | offline `ca skill` inspection and exact profile enablement plus confirmed local install/import/scaffolding | changing manager output, state-store permissions, confirmation, publication, or overwrite behavior |
| `src/mcp.rs` | hash-trusted stdio/Streamable HTTP MCP configuration, lazy clients, dynamic tools/text resources, bounded protocol surfaces, and transport cleanup | changing MCP discovery, execution, resources, trust, reconnect, or lifecycle |
| `src/lsp.rs` | lazy process-scoped LSP JSON-RPC clients, synchronous non-activating status snapshots, versioned post-mutation diagnostics, document synchronization, UTF-16 coordinate conversion, workspace-only result normalization, and process-group cleanup | changing configured semantic-navigation servers, status classification, diagnostic version/bounds, protocol bounds, URI authority, or semantic tool results |
| `src/tool_selection.rs` | process-only replacement/include/exclude policy and restrictive resolution over built-in and dynamic registries | changing model-visible tool selection, runner profile intersections, or unknown-name failures |
| `src/permission.rs` | typed Allow/Ask/Deny policy, legacy adaptation, canonical path globs, safe command prefixes, bounded process-only session grants, and dynamic AutoReview classifiers | changing tool execution policy, AutoReview classification, rule precedence, session grants, or manager-safe permission summaries |
| `src/work.rs` | revisioned plan/task/evidence artifacts, recorded validation events, user decisions, CAS reducer, and branch lineage | changing durable plans, task status/evidence, work ownership, or approval state |
| `src/agent/subagent.rs` | asynchronous child manager, start-time route-spec/runtime-auth resolution, provider runner, stable lifecycle/events, typed origins, bounded sanitized activity watch snapshots, exact grants, approval waits, and child job cleanup | changing delegation limits, provider inheritance/authentication, context, authority, live presentation, control, or persistence summaries |
| `src/review.rs` | isolated model-review prompt/tool loop, typed findings, read-only execution, and terminal review formatting | changing model review context, finding schema, reviewer tools, or review protocol |
| `src/tui/event.rs` | terminal setup/teardown, raw/alternate-screen/bracketed-paste ownership, and crossterm-to-app events | changing terminal modes or event ingestion |
| `src/tui/commands.rs` | slash-command registry, lookup, search, metadata, and typed input actions | adding or changing command identity/discovery/availability |
| `src/tui/input.rs` | input editor, overlays, keybindings, and typed action emission | changing user interaction state machines |
| `src/tui/render.rs` | ratatui layout, conversation/overlay rendering, presentation-only spawn-failure/exploration/edit-command grouping, activity-header hit-region projection, and white blinking composer-cursor placement | changing visible terminal output or focused cursor-cell ownership |
| `src/markdown.rs` | markdown-to-ratatui conversion | changing formatted assistant content |
| `src/runtime.rs` | immutable active-profile/capability resolution plus injected config/data/session/history path policy | changing production runtime locations or profile capabilities |
| `src/doctor.rs` | secret-safe offline readiness reports and explicit provider checks | changing diagnostic checks, output schema, or exit semantics |
| `src/config.rs` | config model and semantic AutoReview agent default, raw `ConfigStore`, safe provider-management snapshots, atomic provider/model/default/route-preset patches and deletion, explicit OpenAI-template constants and template-only protocol persistence, exact model profiles/capabilities | changing config schema/defaults, provider/model/route-preset selection or editing, context limits, or reasoning dialects |
| `src/provider_secrets.rs` | redacted secret input plus canonical `credential://ca/...` resolution over profile-scoped macOS Keychain, hardened portable Unix files, bare process memory, and legacy references | changing direct API-key persistence, resolution, or cleanup |
| `src/provider_connect.rs` | canonical first-class provider definitions, candidate probe/rollback/selection transactions, and shared runtime credential resolution | changing automated `/provider` setup or any provider-client construction boundary |
| `src/mimo_auth.rs` | Xiaomi MiMoCode X25519 authorization URL, AES-GCM result decryption, and bounded multi-connection macOS loopback callback receiver | changing Xiaomi browser authentication or encrypted-result handling |
| `src/codex_auth.rs` | OpenAI Codex browser PKCE/device authorization, refresh/revoke, and hardened shared auth-cache lifecycle | changing ChatGPT subscription authentication or `$CODEX_HOME/auth.json` handling |
| `src/provider_probe.rs` | one-at-a-time, bounded, isolated provider credential checks with typed secret-free outcomes | changing `/provider setup`, probe request shape, timeout/output bounds, or provider health classification |
| `src/model_catalog.rs` | bounded concurrent OpenAI-compatible provider model discovery, provider-specific catalog authentication, and small documented fallbacks | changing `/models` discovery, response validation, fallback policy, or catalog limits |
| `src/provider_discovery.rs` | explicit credentialless loopback-only local-provider scanning with fixed ports, deadlines, concurrency/body/model bounds, and safe candidates | changing `/provider discover`, its network authority, candidate validation, or scan limits |
| `src/fallback.rs` | exact configured provider/model fallback rules, closed safe-cause classification, active-turn routes, and persisted secret-free provenance | changing interactive fallback eligibility, validation, routing identity, or turn provenance |
| `src/agent/stream.rs` | provider events, typed safe errors, complete-turn assembly, and redacted route-bound continuation state | changing canonical runner-facing stream semantics or provider-state identity |
| `src/session.rs` | V1-V4-compatible Session V4 semantics plus immediate local first-request title fallback, bounded generated-title input/output cleanup, and reversible UUID-backed `ses_…` public IDs; optional timed-turn annotations, compact terminal conversation-agent summaries, and bounded foreign-import provenance; storage-v1 metadata/append-only JSONL history bundles; atomic `SessionStore`; bounded recent/page loads; exact-pair rename/deletion/archive/restore; pinning, queued drafts, timeline boundaries, branches, and inert import publication | changing session format/lifecycle |
| `src/session_portability.rs` | bounded normalized session export/import schemas, inert-history validation, and regular-file-only atomic local I/O | changing `/export`, `/import`, portable field policy, or portability bounds |
| `src/session_retention.rs` | validated disabled-by-default retention settings, deterministic exact preview construction, typed deletion reasons, and bounded batch outcomes | changing `/retention` planning, age/count semantics, preview limits, or deletion reporting |
| `src/undo.rs` | bounded process-only file mutation receipts, drift-safe undo/redo transitions, exact regular-file restoration, and redo invalidation | changing `/undo`, `/redo`, or reversible built-in file mutations |
| `src/context.rs` | exact-route estimator identity, conservative local request-category estimates, ordinary and retained-exchange checkpoint boundaries/prompts/reconstruction, deterministic context-map allocation, and shell history | changing project-context attribution, estimation, injection, or compaction |
| `src/changes.rs` | read-only Git status/count/diff loading plus bounded working/base/commit/custom model-review inputs | changing working-tree display or model-review Git data collection |
| `src/change_walkthrough.rs` | complete bounded Git-snapshot eligibility, source hashing, isolated structured explanation, validation, and safe process-only walkthrough state | changing `/walkthrough`, its reporting schema, complete-input boundary, or freshness contract |
| `src/jobs.rs` | process-scoped, run/session-owned shell jobs with sandbox startup, independent head-tail stream captures, typed status, events, stdin, timeout, cancellation, temp cleanup, and shutdown | changing foreground/background process behavior, output, or ownership |
| `src/queue.rs` | serializable stable pending-input ordering/editing, process-only restored-draft eligibility, and turn outcome policy | changing queue identity, persistence, or drain semantics |
| `src/approval.rs` | origin-aware preparation broker, shared rich/legacy/session permission evaluation, and surface-selectable combined or separate prepared network decisions | changing approval origin, prepared policy/network decisions, or prompt routing |
| `src/undo.rs` | bounded pre-mutation file snapshots | changing undo semantics |
| `src/test.rs` | `.ca` parser and scripted runner | changing test DSL or deterministic test infrastructure |

## Interactive Data Flow

1. `main` loads project context/config and initializes `AppState`.
2. `tui::event` emits keyboard, paste, tick, and resize events.
3. `tui::input` mutates editor/overlay state and emits typed actions; `main` handles side effects or detects a submitted prompt.
4. Interactive main-session hooks run through the asynchronous `HookRunner`: SessionStart after the first frame, BeforeSubmitPrompt before canonical append, exact Pre/Post around eligible tool dispatch, and thought/response events at completed provider boundaries. Fail-closed failures open the repairable `/hooks` manager.
5. `start_llm_turn` opens a safe provider/model usage record, establishes the exact primary turn route, builds one owned `PreparedModelRequest`, records its sanitized semantic snapshot, and calls `AgentClient::stream_prepared`.
6. Before any usable event, an exhausted explicitly allowed failure may activate one exact configured fallback. The primary usage span closes failed, the backup opens its own span, and that route remains pinned through every tool continuation without changing the configured current/default model.
7. SSE deltas arrive as internal stream messages and update `StreamingState`.
8. Completion, failure, or cancellation closes the request record exactly once; a completed assistant response is committed to conversation state. An explicit process-only soft-pause request can hold the turn at this completed provider boundary before tools or finalization, or after a completed tool batch before provider continuation. At that same post-batch boundary, the low-yield guard may replace continuation with a typed recovery interaction; Replan supplies one transient next-request instruction, Continue suppresses the guard for that turn, and Cancel remains confirmation-first. `/unpause` consumes an ordinary pause continuation once; cancellation clears all pause/guard state. When a durable goal is active and no tool call remains, the loop starts one fresh evidence evaluator before draining queued input. When a workflow step owns the request, local gates complete/fail that step and the deterministic scheduler starts the next ready node. Separately, an explicitly started approved agent graph schedules only dependency-ready plan tasks through `AgentManager`, observes typed child/tool outcomes, and persists each graph transition.
9. The evaluator receives bounded goal/work evidence and only the non-executable `report_goal_evaluation` schema. A valid `continue` report starts one request-only main-runner continuation; a terminal report stops, while human/approval/error boundaries pause until explicit `/goal resume`.
10. A workflow request receives a request-only step instruction, an exact frozen provider/model route, a restrictive intersection of centrally selected tools, and only the active step's confirmed process-local grants. Dependency outputs are bounded untrusted data; workflow control prompts never become canonical user messages.
11. Pending tool calls are classified for approval and executed.
12. Results must be associated with their tool-call IDs and sent back to the pinned provider route. Workflow state records only safe tool name/status evidence, never arguments or results.
13. The loop repeats until a normal assistant answer, goal/workflow terminal or pause state, cancellation, or error.
14. `tui::render` renders state on every relevant event.

A one-turn route override is process-only state owned by `AppState`. It saves
the original provider/model/reasoning tuple once, updates the parent-agent
route, and is marked consumed only when `start_llm_turn` begins a new main
turn. The authoritative top-level completion boundary restores that original
tuple for Completed, Failed, and Cancelled outcomes before the event loop can
drain queued input. Named route presets persist only provider, model, and
reasoning effort; applying one changes the process route without rewriting the
active profile defaults.

`/init` is a separate workspace-mutation workflow. It captures one bounded,
ignore-aware primary-root evidence snapshot, labels it as untrusted, and sends
exactly one tool-free `ProjectInit` request through the common provider
boundary. The complete validated draft remains process-only until the
Cancel-first preview is explicitly confirmed. Publication rechecks the
canonical root, evidence fingerprint, and target absence, then uses a
descriptor-relative create-new/no-follow write and records the exact
Missing-to-UTF-8 transition in the ordinary undo stack.

`/audit` and `ca audit` share one local read-only scanner. The TUI schedules it
off the event loop and renders the deterministic report in the ordinary pager;
the CLI renders the same report with severity-derived exit status. Traversal is
authority-scoped, no-follow, byte/file/entry/depth/finding bounded, and never
executes configured harness content or contacts a provider/network endpoint.

`/btw` is a deliberately separate provider-request branch. At invocation it freezes the largest protocol-complete canonical prefix plus the submitted side question, starts an independent generation/task/usage span, and passes an empty tool set without consulting hooks, MCP, pasted images, queued drafts, or `StreamingState`. Only process-local `BtwState` receives text deltas; completion, failure, cancellation, or closing the overlay never appends canonical messages or enters session persistence.

`/multi-run` is another explicit process-only branch. Start confirmation creates two to five profile-owned worktrees from one verified Git HEAD, then schedules the same frozen request through `AgentManager` with exact request-local provider/model routes and immutable worktree roots. Candidate traffic uses the central prepared-request, tool, approval, usage, job, and cancellation boundaries. Terminal candidates receive bounded hashed local status/diff evidence; separately confirmed judge and fusion requests are fresh-context and expose only one non-executable structured report schema each. No candidate result changes canonical messages, defaults, the attached workspace, or session state, and every candidate branch/worktree remains user-owned.

`/walkthrough` is a separate process-only explanation branch. It converts one bounded `GitChangeService` capture into a provider-eligible snapshot only when every tracked textual change is present and no untracked input exists, hashes the exact target/file/diff bytes, and sends exactly two isolated messages with only the non-executable `submit_change_walkthrough` schema. The raw diff stays task-local; `AppState` receives only safe source metadata and a validated ordered report. While the completed overlay remains open, a bounded recapture compares the same exact hash and marks the explanation stale without automatically issuing provider traffic. It never mutates canonical messages, sessions, config, Git, approvals, defaults, or the findings-oriented `/review` flow.

## Headless Data Flow

Headless mode validates semantic input before runtime setup, builds provider messages, and delegates to the injectable runner in `src/headless.rs`. The runner consumes the shared typed stream (including explicitly configured reasoning fields), emits text or versioned JSON/JSONL events, authorizes through registry metadata/persistent rules, appends canonical tool results, and maps terminal outcomes to stable exit codes. Structured stdout contains no diagnostics. Ordinary runs retain `ca.headless.v1`; explicitly requesting verification or a local output schema selects `ca.headless.v2`. Verification begins only after main success in a fresh bounded context, defaults to read-only tools, and reserves exit 9 for a valid failed report. Structured output is a sole-final reserved tool call revalidated against the unchanged local schema before emission.

## JSONL Service Data Flow

Only leading `serve` and `attach` tokens select `src/service.rs`; all other invocations remain under the existing flat positional parser. Stdio reads one strict 64-KiB-bounded `ca.serve.v1` operation per line and writes only flushed JSON event envelopes to stdout. WebSocket maps exactly one bounded text frame to each existing operation/event. Its listener and client URLs require literal loopback IPs; handshakes load a protected external file/environment token, reject browser Origin and extra concurrent clients, and retain only a fixed digest while listening. Token values and raw handshake headers never enter args Debug, service state, events, or diagnostics.

`ca attach` authenticates to one loopback endpoint and transparently relays bounded local JSONL, keeping protocol stdout separate from safe stderr diagnostics. A completed or disconnected client releases server ownership; reconnect creates a fresh service identity and sequence. Non-loopback/TLS listeners and SSH orchestration are absent by design; remote users supply their own SSH tunnel terminating on loopback.

One startup-bounded, process-only service session owns an exact configured provider/model, workspace, central tool-selection result, prepared-request history, extension clients, and `JobOwner`; it never constructs a `SessionStore`. The released wire vocabulary is represented by public `ServeOp` and `ServeEvent` enums; their serde names remain the unchanged `ca.serve.v1` operation/event shapes.

## ACP v1 Data Flow

Only a leading `acp` token selects `src/acp.rs`. Stdio carries one newline-
delimited JSON-RPC 2.0 message at a time through the pinned official
`agent-client-protocol=2.0.0` SDK with experimental protocol-v2 features
disabled. Initialization accepts stable wire version 1 only. The adapter owns
per-connection CA sessions and translates new/load/prompt/cancel/set-mode into
the same local workspace, provider, tool-selection, approval, usage, job, MCP,
LSP, and cancellation boundaries used by the terminal agent.

ACP advertises agent, plan, and ask modes plus text, thought, tool, plan,
available-command, usage, and mode updates. Client permission requests are the
only ACP authority path; absent, malformed, timed-out, disconnected, or denied
answers fail mutating/network work closed. Client filesystem, terminal, and MCP
methods are not authority inputs. Locations are absolute, workspace-contained,
and one-based. EOF and transport failure cancel and join active turns, close
extensions/jobs, and retain no privileged secret. `ca.serve.v1` remains a byte-
independent JSONL/WebSocket protocol with its own authentication and operations.

Each `user_input` builds `PreparedModelRequest::Service`, emits nested canonical headless runner events, and retains one complete tool-call/result loop. Persistent Deny is automatic; otherwise unresolved work emits an exact approval or structured-interaction request and cannot continue until a matching client response arrives. Cancellation, replacement, EOF, WebSocket close, attach exit, shutdown, client disconnect, and internal failure abort the active task and close only the session's extensions and owned processes. Runtime endpoints, headers, credentials, response bodies, and binary data never enter operation state, event envelopes, snapshots, or Debug output.

## Scheduled Data Flow

Only the leading `schedule` token selects `src/schedule.rs`; ordinary positional prompts remain unchanged. Add validates one bounded local TOML/JSON definition against the selected profile, captures an exact workflow ID/digest when applicable, and persists the record disabled under the profile data root. A profile lock serializes every mutation and invocation; `tick` claims at most one due item, persists one-shot disablement or the first future cron occurrence before provider work, and records overdue work as skipped without replay.

Execution sends one frozen `WorkerSpec` over bounded stdin to the current binary. The child builds `PreparedModelRequest::Scheduled` for the exact configured provider/model and canonical workspace with no sessions, skills, MCP, LSP, hooks, memory, fallback, or interactive approval. Its visible tools are the exact declared built-ins, process grants cannot exceed that set, persistent Deny/Ask remains stronger, and provider-tool network requires both stored and invocation authority. Workflow jobs revalidate the captured digest and execute deterministic ready steps with dependency outputs, local evidence gates, configured retries, and schedule-level route/tool/grant intersections.

`ScheduleDefinition` is the public strict serialized definition boundary; making that name public does not expose mutation or execution authority.

The parent enforces the total runtime deadline, kills/reaps the owned process group, retains only safe typed outcome metadata, and delivers bounded output either by symlink-safe atomic workspace-relative replacement or an explicitly enabled signed webhook. Webhooks require HTTPS or literal-loopback HTTP, an environment secret, HMAC-SHA256, no proxy/redirects/URL authority, and discard response bodies. No command installs or edits cron/launchd state.

## Local Plugin Data Flow

Only the leading `plugin` token selects `src/plugin.rs`; ordinary prompts remain unchanged. `add` accepts one bounded strict local TOML/JSON manifest, canonicalizes every declared skill tree and workflow inside its non-symlink bundle root, validates existing MCP/hook/workflow/theme schemas, computes a deterministic content digest, and writes a disabled record to an owner-only profile registry. Plugin MCP credentials may only be exact `${ENV_NAME}` references. List/show output derives safe contribution names, capabilities, collisions, and integrity without displaying hook/MCP commands, headers, prompts, or resolved values.

`enable ID --confirm ID` rechecks the digest and exact MCP/hook/workflow/theme/skill identities against the active profile, user skill root, and other enabled bundles before the next launch can compose anything. Runtime composition repeats those checks, appends skill roots, MCP definitions, and hooks only through their existing managers, and exposes only namespaced workflow paths and aliases to built-in themes. Bare mode never reads or mutates the plugin registry. Disable/remove never stop running work, delete source bundles, grant approval, or introduce native/in-process executable code.

`PluginManifest` is the public strict local bundle definition boundary; registry records, persistence, and activation remain private and confirmation-gated.

`plugin catalog` adds a separate local-snapshot trust boundary in `src/plugin_catalog.rs`. An exact-confirmed trust command pins one local Ed25519 public key; sync verifies the signature over exact payload bytes and only accepts a monotonic bounded catalog revision. Signed entries carry one plugin version, package-relative path, and SHA-256. Install/update recheck the package and every decoded file hash, build a private staging tree, validate the ordinary PLG-084 manifest, atomically move the complete package under profile data, and register it disabled. The original signed package is retained inside the installed tree so rollback can revalidate the full hash and exact file set before switching the disabled registry record. No catalog operation performs network I/O, enables a plugin, replaces a local-source plugin, or deletes retained versions automatically.

Only a leading `runtime` token selects the persistent manager in `src/external_runtime.rs`; positional prompts remain unchanged and bare rejects the command before path discovery. Trust pins one exact HTTPS base path—or literal-loopback HTTP for local testing—to one Ed25519 root. `sync` is the only catalog request and install/update are the only artifact requests; all use no proxy, no redirects, no URL credentials/query/fragment, bounded deadlines, and no response-body errors. A closed signed entry declares one target, license label, exact file paths/sources/sizes/SHA-256 hashes/executable bits, and named entrypoints. Each file streams into an owner-only generation staging tree, is rehashed before atomic promotion, and is never executed. Rollback/path/remove revalidate the complete receipt, file set, modes, sizes, hashes, and containment. Installed runtimes remain inert by default; LOC-089 is the sole consumer and resolves only the exact `llama-server` entrypoint after an explicit local-run command.

Only a leading `local` token selects the persistent GGUF manager in `src/local_models.rs`. Import accepts one user-selected regular non-symlink GGUF, validates a bounded versioned header, streams exact bytes/SHA-256 into private profile staging, and preserves the source. Download requires one exact HTTPS or literal-loopback URL, declared bytes/hash/license, and exact confirmation; it uses no proxy or redirects. Inspect and launch reverify the complete one-file tree and receipt before returning any path. Remove renames and deletes only the exact manager-owned tree after confirmation.

`--local-model ID --local-runtime ID` is an explicit process-only run mode. It resolves the GGUF and the EXT-088 `llama-server` entrypoint immediately before spawn, supplies fixed literal-loopback/model/alias/context/Jinja arguments, clears the environment, requires the OS sandbox, bounds independent stdout/stderr head-and-tail logs, waits through 503 loading to valid `/health` and `/v1/models`, and then injects one credentialless Chat Completions provider only into the in-memory config. Local runs create neither config nor sessions when absent and never persist the provider/default. Normal completion, setup failure, cancellation, and guard drop stop the full owned process group. The current macOS sandbox grants network as one broad capability, so fixed loopback binding plus the explicit launch remains the authority boundary; `ca` does not claim sandbox-enforced egress restriction for llama-server.

Only a leading `update` token selects the global binary lifecycle in `src/self_update.rs`; positional prompts remain unchanged. Trust pins one exact HTTPS base path—or literal-loopback HTTP in tests—and one Ed25519 root without network access, and refuses implicit authority rotation. `check` fetches only `releases.json`, verifies exact signed payload bytes, monotonic revision, closed channel/version data, and the current `os-arch` target. `download` is separately confirmed and streams one same-origin declared artifact through exact byte/SHA-256 bounds into owner-only retained storage without execution.

Install accepts only staged bytes that still exactly match the selected signed release, canonicalizes one current-user-owned non-symlink invoking executable, rejects group/other-writable targets, retains a verified private backup, writes a private same-directory temporary, fsyncs, and atomically renames over exactly that executable. Rollback uses the same checks and retains the replaced version as the next rollback. State-save failures restore executable bytes before returning. No updater command discovers PATH targets, uses proxies/redirects, prints response bodies, or checks/downloads/installs automatically.

## Critical Invariants

### Provider protocol

- Every assistant tool call has a stable ID, name, and complete JSON arguments.
- Every executed or denied call produces exactly one result tied to that ID before the next provider turn.
- Stream fragments are assembled deterministically; malformed SSE or JSON surfaces an error rather than invented content.
- Provider-specific retry/refusal handling is isolated from UI state.
- Provider exchange tracing is explicit and default-off. One process hub observes
  prepared model traffic only at the common `AgentClient` transport boundary;
  trace failure cannot fail, retry, delay, or otherwise alter a provider turn.
- Every trace activation owns one versioned append-only JSONL file below the
  active profile data directory. Accepted records have one monotonic sequence,
  the writer is bounded, files cap at 16 MiB, storage is owner-only on Unix,
  bare profiles cannot persist, and retention ends only at exact confirmed
  deletion.
- Trace redaction removes URL userinfo/query values, credentials, cookies,
  account identifiers, configured/runtime secrets, encrypted reasoning, binary
  media, secret-shaped JSON, and terminal controls. Trace records remain outside
  sessions, exports, canonical history, `/request`, usage, and provider
  continuation state.
- Doctor reports consume only raw safe provider summaries unless `--connect` is explicit. Resolved credentials remain inside the existing provider-probe task and only typed outcomes return.
- Headless JSONL envelopes have one stable run ID and monotonic sequence; aggregate JSON contains the same ordered event stream.
- `ca.headless.v1` is unchanged when no new contract flag is present. V2-only check/structured events never enter v1 output, and schemas that are malformed, oversized, non-object, symlinked, or remotely referenced fail before provider traffic.
- TUI provider requests create one process-only usage record before I/O. The record contains only kind, provider/model IDs, duration, typed outcome, and optional token counts; prompts, output, endpoints, and credentials never enter the ledger.
- Main turns, tool continuations, checkpoints, reviews, provider-backed subagents, and explicit provider probes use the same recorder boundary. Dropping an in-flight background span records cancellation rather than success or failure.
- Side questions use the same provider client and typed stream validator but a separate task/generation and `UsageKind::Btw` span. Their state contains safe provider/model IDs plus question/answer/status only; resolved provider configuration remains task-local.
- Interactive main/continuation, side-question, checkpoint, review, subagent, headless, scripted, and provider-probe runners construct `PreparedModelRequest` immediately before I/O. The default Chat Completions adapter serializes the same model/messages/tools/options as before; endpoints, headers, and credentials are adapter-owned runtime values.
- `ProviderProtocol` is defaulted at config decode. An absent field is exactly Chat Completions and does not rewrite TOML; complete adapters currently accept `chat_completions`, opt-in `openai_responses`, and opt-in `anthropic_messages`. `AgentClient` selects one adapter for endpoint suffix, authentication, exact reusable request bytes, and stream decoding, while every runner sees only `PreparedModelRequest`, `ProviderEvent`, and typed safe failures.
- The OpenAI provider template carries a process-only `OpenAiResponses` override from its distinct editor request into one atomic new-provider save. Generic Add Provider carries no override, existing-provider/model edits cannot set one and preserve their raw protocol entry, and duplicate `openai` creation refuses overwrite before editor mutation or persistence.
- Chat Completions, OpenAI Responses, and Anthropic Messages SSE framing plus JSON/usage/tool-fragment translation live in `src/agent/provider.rs`. `src/provider_contract.rs` adds first-party behavior only when provider ID, exact HTTPS endpoint, protocol, and exact model all match an official OpenAI/Codex, MiMo, or DeepSeek route; named proxies remain generic. Direct OpenAI Responses sends exact effort, `store:false`, encrypted-reasoning inclusion, and direct-only cache controls. MiMo Chat owns `thinking.type` plus `reasoning_content` replay, MiMo Responses maps disabled/high reasoning without OpenAI-only fields, DeepSeek Chat owns its documented thinking/effort mapping, and DeepSeek Responses excludes unsupported store/include/cache fields. Completed Responses items remain caller-replayed in `output_index` order without `previous_response_id`. Anthropic moves leading system/developer text to its top-level system blocks, maps assistant calls/results to `tool_use`/coalesced `tool_result`, uses versioned conditional `x-api-key` authentication, and requires a valid Messages lifecycle through `message_stop`. Provider failures discard incomplete state. `src/agent/stream.rs` retains only bounded, payload-redacted continuation items bound to `(provider, protocol, normalized endpoint SHA-256)`; raw endpoints, headers, credentials, cache keys, top-level response IDs, and remote conversation IDs remain absent.
- Canonical message content is typed text or normalized image content above provider adapters. Only exact image-capable model profiles receive image blocks; adapters own their wire shape, text-only requests keep their characterized bytes, and incompatible images fail before provider I/O. Clipboard and `view_image` bytes are bounded, process-only, and available to one request/continuation; durable messages, snapshots, hooks, and structured output receive only unavailable-safe MIME/dimension markers.
- User and workspace memory remain independently controllable. Missing recall
  and proposal-generation switches now adopt the enabled learning default,
  while an explicit `false` remains authoritative and contributes zero prompt
  bytes or proposal tools. Enabled recall injects deterministic, independently
  capped indexes that explicitly mark paths, versions, and facts stale until
  reverified. Ordinary model output can only create a process-local validated
  proposal, and persistence requires explicit Cancel-first `/memory`
  acceptance. Dream uses a separate provenance- and digest-bound journal and
  can mutate only prior Dream-owned records.
- Goal evaluation is fresh-context and read-only: it receives no canonical history, executable tools, skills, memory, MCP, hooks, images, endpoint, or credentials. Its sole reporting schema cannot dispatch, and malformed or stale results pause rather than certify success.
- Goal continuation feedback is request-only and never becomes a synthetic canonical user message. Session V4 persists only bounded typed goal state and evaluator summaries; full forks get a new goal identity and historical forks omit the goal.
- Goal budgets count reported main-turn and evaluator provider tokens against the request that began while the goal owned it, including an in-flight request paused before completion. Iteration, token, and working-time limits are finite; approvals, interactions, steering, cancellation, failures, and restored active goals require explicit resume.
- Workflow definitions are bounded local data, not executable authority. Validation rejects unknown fields, traversal/symlinks, cycles, invalid routes/tools/grants/retries/evidence, and more than 64 steps before provider traffic. Scheduling is sequential and deterministic in definition order.
- Workflow tool exposure is only a restrictive intersection of the central resolver. Confirmed exact-tool/path/command grants are process-local and remain below persistent Deny/Ask policy; network and broad authority cannot be declared by a workflow file.
- Session V4 persists only bounded typed workflow run state. Restored running runs pause, historical branches omit workflow state, full forks receive a fresh run identity, and user/approval/hook/error/cancellation boundaries require explicit `/workflow resume`. Goals and workflows are mutually exclusive main-loop owners.
- One shared `RequestStore` retains only the latest already-sanitized semantic snapshot. Exact runtime credentials are applied only as redaction inputs and never enter the store; binary payloads, terminal controls, credential-shaped fields, deeply nested JSON, item counts, and rendered text are bounded before `/request` can observe them.
- Interactive fallback is opt-in through exact primary and backup provider/model pairs. Only exhausted connect/timeout failures, HTTP 429, and HTTP 5xx are classifiable; 401/403/404, missing credentials, request/protocol/content/refusal failures, and any failure after nonempty text/reasoning or a tool-call event never switch providers.
- A turn can activate at most one fallback route. The route is fixed for the complete tool loop, a backup failure cannot chain, and completion/failure/cancellation clears it so the next submitted user turn starts from `current_model`. Headless, scripted, check, review, checkpoint, subagent, probe, and `/btw` requests remain exact-provider workflows.

### Approvals and mutations

- Read-only classification is explicit; unknown tools default to the safer policy.
- A mutating tool cannot execute before its approval decision.
- Tool path/cwd resources are resolved against workspace roots before policy; approval and execution consume the same prepared arguments.
- Persistent allow and yolo alone never widen workspace roots, required sandbox policy, or network authority; persistent deny takes precedence.
- Tool schemas, validation, handler dispatch, profiles, and mutation classification come from one registry spec; unknown tools fail closed.
- `read_pdf` stays inside ordinary read-only workspace authority: it accepts only regular `.pdf` files up to 32 MiB, parses owned bytes on a blocking worker, extracts at most 20 selected pages/56 KiB of text, reports bounded continuation metadata, rejects encrypted/malformed/over-10,000-page documents, and never traverses embedded files or actions.
- `view_image` stays inside ordinary read-only workspace authority and central tool selection. It accepts only regular allowlisted raster files for exact image-capable models, checks compressed/pixel/decoded/normalized caps on a blocking worker, strips metadata by pixel decode/re-encode, and supplies bytes only to the immediate provider continuation.
- `browser` is absent from default tool sets and appears only through an exact `--tools` or `--include-tools` selection. Every action remains mutating/approval-gated; only `navigate` and `download` request the distinct one-call network grant. One owner-bound process uses a new temporary profile and literal-loopback CDP, blocks external HTTP(S)/WS(S) outside the approved network window, caps runtime/frames/DOM/images/downloads, creates approved workspace files without overwrite, and kills its process group on owner/process cleanup.
- Voice input is disabled by default and captures from the macOS default input device only after the visible `/voice` modal receives Space. One generation owns the bounded mono WAV in a process-owned temporary directory; cancellation, timeout, completion, and shutdown discard it. Local transcription uses one configured absolute executable without a shell inside a required network-denied sandbox, while provider transcription uses one exact configured provider/model and the resolved credential only inside the worker. The bounded transcript enters the composer as an editable unsent draft and never enters canonical messages, sessions, snapshots, or provider history automatically.
- Every runner resolves model-visible tools through `ToolSelectionPolicy`: profile eligibility is the upper bound, replacement applies first, inclusion second, exclusion last, and runner/model/capability/grant restrictions can only remove tools. Final resolution occurs after local MCP catalog assembly and before request preparation/provider I/O.
- Selection is exposure, not authority. A tool hidden from the prepared request
  is never executed if a provider nevertheless calls it. In the interactive
  loop, the first such call in one logical user turn becomes a bounded resolved
  error and permits one ordinary provider continuation; repetition fails at
  the existing `/retry` boundary, and workflows retain explicit failure. A
  selected tool still traverses normal argument validation, workspace
  preparation, approval, sandbox, network, and job-owner checks.
- Every interactive, headless, MCP, and provider-subagent execution evaluates one typed permission policy after validation and canonical resource preparation. Deny wins over a matching process-only session grant, which wins over Ask, which wins over Allow; legacy rules are adapted in memory without rewriting their TOML.
- Safe command-prefix rules apply only to a single parsed command. Shell operators, redirection, expansion, substitution, escapes, control characters, or malformed quoting force an explicit prompt unless a broad Deny already blocks the call. Neither yolo nor a session grant bypasses that fallback.
- Ordinary TUI built-ins make one interactive decision over the prepared tool invocation and its requested network capability: Allow/AutoReview/RunEverything carry that capability into execution, Ask shows one tool prompt, and Deny blocks. Headless, service, MCP, child, email, and privilege surfaces retain their own stricter contracts. No decision enlarges workspace roots, weakens sandboxing, or exposes a hidden tool.
- Distinct provider call IDs execute independently even when name and arguments match.
- Interactions and approvals share original call order; later automatic tools wait behind unresolved manual items and continuation occurs only when the ordered batch is empty.
- Model interaction answers are tool results, never synthetic user messages. Host secret handles are not serializable/model-visible and zeroize their buffers.
- Portable skill metadata is ambient but bounded; instruction bodies enter a request only through an exact task load or persistent activation. Duplicate names never select by precedence, and declared tools/dependencies never widen tool, workspace, sandbox, MCP, process, or network authority.
- `skill_view` is read-only: every read revalidates the immutable instance/full-tree digest, containment, regular type, and byte bounds. Reading a script does not execute it.
- Skill suggestions rank only already-discovered safe metadata and remain host-visible/process-only. Alias and lexical matches always name the canonical ID but never activate, load a body, enter a request/session, or bypass the existing hash/approval/budget boundary.
- MCP dynamic tools are schema-validated registry entries carrying their server origin; annotations never authorize execution and built-ins cannot be shadowed.
- Project MCP configuration is inactive until host config matches canonical workspace and SHA-256. Network needs trusted config plus a distinct run/server grant.
- Approval is exactly once. Denial is represented as a tool result so the model can continue coherently.
- Undo snapshots are taken before eligible filesystem mutation, not after.

### TUI

- Rendering is a pure view of `AppState` wherever practical.
- Ordinary composer drafts do not trigger passive skill ranking or replace the
  status row. Skill ranking is an explicit `/skills <query>` surface.
- Input modes and overlays have explicit precedence; one event cannot activate two actions.
- Bracketed paste is active only while the TUI owns raw alternate-screen input;
  dropped/pasted paths arrive as one paste event, and lifecycle cleanup disables
  paste before leaving the alternate screen or raw mode.
- The focused composer resolves one UTF-8-safe display-cell insertion point
  through the shared layout, hides the terminal-owned cursor, and alternates
  that cell between a white software block and its untouched rendered style on
  a stable 500 ms phase. Keyboard input and paste reset the white phase; modal
  focus suppresses the composer cursor.
- An active assistant response has exactly one stream cursor at the end of its
  final non-empty rendered line. Visible and hidden phases retain the same
  symbol and cell width and differ only in style; animation alone cannot alter
  wrapping, transcript maximum, bottom-relative position, visible top row, or
  the conversation/composer boundary.
- Busy Enter appends one stable queue item; only a successful fully resolved turn drains one item.
- `TurnLifecycle` separates active phase from terminal outcome. Sending, Waiting, Reasoning, Responding, Tool, Interaction, and process-only Paused are active phases; only `TurnOutcome::Completed` authorizes queue drain. `/pause` never interrupts work in flight: it is honored only before tools, before provider continuation, or before plain-response finalization.
- Reasoning request fields and stream fields exist only for an exact configured model profile. Unknown models receive no inferred effort or provider-specific option.
- Canonical messages retain visible reasoning separately in `Message::thinking`; rendering is collapsed unless `/thinking` enables details. `Message::response_items` is non-rendered provider continuation state, committed only with a successful completed assistant turn and discarded on cancellation or failure.
- Steering removes unresolved tool-call metadata before starting a new provider generation.
- Cursor indices remain valid UTF-8 boundaries.
- The advertised `Ctrl+K` model-picker path shares setup with `/model`, opens on the active model, and preserves composer text. Filtered selection is bounded and long catalogs render through a fixed visible window rather than displacing picker controls.
- Model preferences are presentation metadata, not route authority. Persistent profiles store only bounded provider/model identities, favorite membership, and recent-selection timestamps; unavailable identities stay dormant. The picker projects unique Favorites, Recent, and provider groups, while bare mode loads no preference file, persists no favorites, and tracks at most eight recent selections in process memory.
- Foreign transcript import is reconstruction, not execution-state migration. Only explicit user-selected sources are parsed off the TUI loop. Closed adapters retain bounded visible user/assistant text and safe timestamps, assign fresh UUIDs, use the current route on later resume, and persist filename-plus-digest provenance with learning disabled unless explicitly selected. Preview performs no writes; confirmation never replaces the active session or mutates the source.
- Picker/editor paste follows overlay precedence: pasted model/filter text updates the visible control and cannot mutate the hidden composer. Provider-editor fields own UTF-8-safe cursors with insertion, deletion, field traversal, and cursor-centered rendering for long values.
- `/provider` and `/providers` open a provider-first manager. Provider-list/detail/editor/delete-confirmation states have explicit return paths; provider IDs are fixed, individual model deletion is absent, and provider mutations lock their visible surface while blocking work runs. Provider and setup lists expose a distinct `Shift+O` OpenAI template beside the unchanged custom Add Provider path; opening the editable masked form performs no I/O, and only Enter save persists its exact safe fields and Responses marker.
- Exact `/provider discover` and `/providers discover` open a separate process-only local-discovery overlay. Scan, rescan, stale-generation rejection, and Add Provider return paths never touch the composer or configured-provider manager state.
- `/doctor` is an idle-only read-only command that opens the same offline readiness report as `ca --doctor`; it never initiates provider traffic.
- `/usage` is read-only and remains available while a turn is active. It renders a point-in-time local ledger snapshot without holding the shared lock while drawing, then explicitly queries all configured supported provider-account adapters concurrently. OpenAI organization Usage/Costs and DeepSeek balance calls use only configured credentials bound to their exact official hosts, ten-second one-attempt bounds, fixed body limits, and a 60-second process cache; MiMo reports only configured Token Plan allocation and locally observed consumption. These incompatible scopes are labeled separately, and the non-model account calls cannot enter provider traces, sessions, exports, canonical history, or the usage ledger itself.
- `/request` is read-only and available while work is active. It renders one immutable point-in-time snapshot or a no-request explanation; opening it never starts MCP, reads config/files, or initiates provider traffic.
- `/btw [question]` is `Always` and owns a process-only editor/answer overlay. Esc aborts only the BTW task; Ctrl+C retains its existing main-turn cancellation meaning. Stale BTW generations and results for a closed/non-running overlay are ignored.
- `/keymap [filter]` exposes only the bounded global composer actions defined by `KeyAction`, including always-available `copy_transcript` (default `Ctrl+X C`). Ctrl+R Diff, Ctrl+C/Ctrl+D process controls, Enter/Esc, editor navigation, approval decisions, confirmations, and modal controls remain outside the resolver. Pending prefixes time out, cannot absorb emergency controls, and dispatch exactly one action.
- `/history [filter]` searches newest-first process input history without shell/session persistence. Selecting a row restores an editable draft and never submits it; overlay paste cannot reach the composer.
- `/hooks [filter]` exposes only safe hook metadata and typed last results. Toggle, reload, test, cancellation, and startup repair run asynchronously; commands, payloads, stdout/stderr, and resolved environment values never enter `AppState` or snapshots. Bare mode remains inspect-only.
- `/config [filter]` owns a safe searchable navigation overlay. Complex provider/model/MCP/skill actions route through their existing typed managers. “Delegated agent” reuses the configured-model picker, with an explicit inherit row and one optional exact cross-provider route; review and delegation route pairs are patched atomically without changing chat defaults.
- `/theme` previews one closed built-in palette and persists only on Enter; the
  opt-in `mimocode` palette adds warm-orange actions, periwinkle headings,
  pale-green Plan identity, and dark surfaces without changing the default.
  `/statusline` toggles and reorders one duplicate-free closed field list.
  Missing `[ui]` config reproduces the legacy title and idle-status output
  exactly, and bare mode remains preview-only.
- Assistant Markdown is sanitized before theme-aware rendering. Ordered lists
  retain their source numbering and tables are terminal-width-bounded full
  grids. Clipboard feedback is a short process-only toast containing only a
  fixed result message, never the copied content. Plan mode uses distinct
  composer presentation while retaining its existing read-only tools and
  explicit approval transition.
- Custom status command text remains config-file-only and never enters `AppState`, snapshots, sessions, requests, or diagnostics. Its worker runs at most one generation with a one-second deadline, a process group, a required network-denied sandbox, a cleared environment, and bounded sanitized stdout; unavailable or failed enforcement is a safe typed failure.
- `/retention` owns an Overview/Edit/Preview state machine. Saving rules never deletes files; exact previews are Cancel-first, exclude current/pinned/archived sessions, and lock all actions while blocking config/filesystem work runs.
- `/provider setup [filter]` is explicit and process-local. Opening it performs no provider request; one highlighted provider may be checked at a time, cancellation/edit generations reject stale results, and only a passing provider/model pair can be persisted from the finish chooser.
- Long work never blocks event polling or drawing.
- Terminal teardown runs on normal exit and error paths.

### Hook execution

- Hook commands remain config-file-only. `HookRunner` owns command-bearing runtime values internally and exposes redacted custom Debug implementations plus command-free management snapshots.
- Every child has a dedicated temporary directory and process group. Timeout, cancellation, task abort, panic, and shutdown terminate descendants and clean temporary state; payloads and output are bounded.
- An agent graph never executes through a separate provider/tool runner. `AgentManager` remains authoritative for concurrency, per-turn child limits, exact grants, workspace snapshots, approval waits, timeout, cancellation, and cleanup. Worktree-bound tasks resolve one exact profile-owned `WorkspaceContext` without attaching or moving the main session.
- Graph attempts are append-only and bounded. Descendants remain dependency-blocked after local preflight, child, or evidence failure until the user approves a specific retry; restore converts live work to interrupted/paused and never auto-spawns.
- Hook processes use the effective sandbox policy with network denied. Policy-only/off modes are reported truthfully and do not claim OS-level network isolation.
- HOK-052 covers only interactive main-session lifecycle events. Provider checks, `/btw`, checkpoint, review, subagent, headless/scripted, MCP-internal, and user `/shell` work do not execute user hooks.

### Persistence

- Config and session format changes are backward-compatible or migrated explicitly.
- File undo/redo is deliberately not persistence. Only successful main-session built-in UTF-8 file mutations create bounded before/after receipts; receipt content is excluded from Debug/model/hooks/requests/sessions. Undo/redo compare the live exact state and perform one blocking-worker restoration before moving an entry between bounded process-only stacks. New mutations clear redo; `/rewind`, `/retry`, `/fork`, and Session V4 remain independent.
- Runtime paths are discovered once at startup and injected; missing `HOME` never falls back to cwd or a developer path.
- `ActiveProfile` is resolved before doctor, model listing, headless, or TUI construction. Default paths remain unchanged; named profiles use whole config files and session roots below `profiles/<name>`, with no overlay behavior.
- Missing named profiles use defaults in memory and do not create a file until a successful mutation. Named Keychain references/accounts include the profile and provider, so deleting one profile's provider cannot affect another profile's credential.
- `bare` reads the normal config but cannot persist configuration or sessions. MCP, skills and their activation tool, hooks, and session persistence are absent; model selection, retry, rewind, and branching remain process-only.
- `--no-session` disables only session persistence for the run. It conflicts with every startup resume path, does not read or create a session root, leaves config/MCP/skills/hooks enabled, and keeps retry, rewind, forks, current identity, and queued drafts in memory.
- Direct Session V4/storage-v1 persistence preserves optional defaulted provider state, timed turn metadata, terminal child summaries, and bounded foreign-import provenance across full loads, lazy pages, and branches without changing old record bytes. Checkpoint prompts, request views, and portable export/import omit opaque state. `/export` emits a bounded Markdown document or deny-unknown-fields `ca.session-export.v1` JSON containing normalized conversation data only; `/import <path>` accepts that bounded local regular JSON, rejects unresolved/executable tool state, and assigns a fresh UUID/current defaults. Foreign `/import codex|claude|opencode <path>` adds only visible inert history plus safe timestamps and non-path provenance after explicit preview confirmation. Neither path carries workspace authority, extension state, approvals, grants, queue, provider state, raw tool output, or runtime agent state.
- Export replacement is explicit and Cancel-first. Missing destinations use a no-overwrite publication step, confirmed replacement uses same-directory atomic rename, and symlink/non-regular targets fail closed. Import/export work stays off the TUI event loop; stale generations and failures cannot change the current session.
- Resolved provider secrets are runtime-only. New direct credentials use exact `credential://ca/provider/<provider>` or `credential://ca/profile/<profile>/provider/<provider>` references; `${SECRET}` placeholders and legacy `keychain://ca/...` references remain readable. macOS resolves canonical references through Keychain. Other Unix targets use owner-only, no-follow, bounded, atomic profile-data files; bare keeps candidates only in memory and cannot become the persisted default.
- Automatic delegation pricing is owned by `src/pricing.rs`: source-bound
  OpenAI, MiMo, and DeepSeek cards use exact rational arithmetic, 30-day
  freshness, and all-or-nothing allowlisted refresh. Catalog v2 distinguishes
  MiMo PAYG metering from Token Plan credits and upgrades v1 catalogs only in
  memory until an explicit refresh; Codex subscription routes remain
  price-ineligible. DeepSeek is metered USD
  only at the exact official root or `/v1`; Flash has a `simple` ceiling and
  Pro a `standard` ceiling. Actual DeepSeek input cost requires consistent
  cache-hit/cache-miss subsets. Legacy user pricing remains separate and
  cannot establish automatic-route eligibility.
- Optional `[keymap]` entries are profile-scoped, validated as a whole before startup or atomic mutation, and preserve unrelated TOML/comments. Missing actions inherit defaults; bare mode may inspect but cannot persist bindings. Input history remains process-only regardless of profile. Composer attachment removal, last-queue editing, and transcript oldest/latest jumps have configurable defaults without changing the emergency controls.
- Optional `[ui]` entries are profile-scoped, validated before activation, and patched atomically without rewriting unrelated TOML/comments. Named profiles remain lazy and isolated; bare reads the normal UI choices but disables custom-command execution and all persistence. `mouse = "scroll"` is the absent-field default: wheel/trackpad events navigate the focused surface and left-drag selects bounded visible transcript cells for in-process copy on release. `mouse = "off"` returns mouse ownership to the terminal without changing the persisted shape.
- `AppState::input` remains the composer text source of truth. Process-only `ComposerUiState` owns grapheme-aware visual rows, a seven-row viewport, preferred display column, history sidecars, registered atomic ranges, exact large-paste payloads, and token-scoped completion state. Rendering and navigation share this layout, so hard newlines, soft wraps, combining sequences, wide glyphs, Home/End, Vim motion, and the white blinking cursor agree.
- The normal session UUID is allocated before the first provider turn. Empty
  titles derive locally from the first user request and never replace an
  explicit name. Public `ses_…` IDs are fixed-width reversible renderings of
  that UUID, not new persisted authority; UUIDs and legacy storage keys remain
  accepted, while `-s` and session UI surfaces prefer the public form.
- Slash, attachment, and one-level no-follow path completion are composer-anchored and generation-tagged. Slash-command completion is valid only for a leading token beginning at byte zero. Inline path discovery requires an explicit `./`, `../`, `~/`, or absolute `/` prefix, but a later absolute path in prose never becomes a command and an empty completion result cannot consume Enter. Tab retains explicit bare-relative-path completion. Up/Down select path results while Left/Right retain grapheme-aware composer editing without restarting an unchanged discovery generation. A shared Unicode-safe list viewport/row layout supplies wraparound arrows, paging, persistent selection visibility, counts, truncation, and narrow stacked descriptions to these surfaces and the model, session, provider-list, and permission pickers.
- Transcript position is process-only and bottom-relative. Ratatui's rendered paragraph row count supplies the exact maximum; incoming output preserves a scrolled viewport and increments unseen feedback, offset zero resumes following, and lazy history prepend retains the visible anchor. Selection points use document rows and cache a bounded sequence of traversed rendered cells, so forward/reverse wheel or edge scrolling preserves Unicode ranges across frames. Resize or incompatible geometry cancels safely, and the 200,000-cell ceiling directs the user to `/copy all`.
- `/copy` and `/copy last` select the latest non-empty assistant response, preferring active streaming text. `/copy all` hydrates complete saved history and reuses the semantic conversation projection without transient chrome or cursor cells. It includes displayed Markdown, tools/diffs, active streaming, and enabled visible reasoning, but excludes system prompts, hidden reasoning, provider-only state, modal/status/composer text, and undisplayed private/raw data.
- The shared pager owns a separate rendered-content selection snapshot using the same document-row traversal. Mouse release copies the complete traversed range, while C copies the complete semantic pager text including off-screen lines; borders, navigation hints, and the covered conversation never enter either clipboard payload. Clipboard failure retains the selection. Learning pagers add Enter actions and explicit refresh without changing pager navigation or close keys.
- `TerminalLifecycle` acquires raw mode, alternate screen, bracketed paste, and optional best-effort mouse capture in order and releases acquired modes in reverse. Runtime UI changes, suspend/resume, partial failure, panic unwinding, unsupported terminals, and Drop share that ownership. Vertical wheel events retain focused navigation; left down/drag/up drives application-native transcript selection; other clicks, motion, and horizontal gestures are ignored.
- The provider/model editor has explicit Add Provider, Add Model, and Edit Model modes. Add Model reuses the selected provider, while existing provider IDs remain fixed; base URL, masked direct API key, API-key environment name, and Model ID are editable. Direct input stays in a redacted zeroizing buffer outside `InputAction` and provider/model drafts, and macOS Keychain/config persistence runs on a blocking worker. The injected secret-store boundary resolves only an exact provider-scoped reference; failed Keychain writes restore the original raw config. Runtime selection refreshes only after a successful save, and editing a discovered-only row creates a prefilled add-to-provider draft because no raw model entry exists to rename yet.
- Provider-manager state is built from raw config summaries containing only provider/model IDs, a credential-stripped endpoint, and credential-source classification. `src/provider_templates.rs` independently supplies a compiled searchable 40-row catalog with safe static metadata and typed availability; opening/filtering/canceling it performs no I/O. Fourteen ready rows can enter the existing secure editor, while 26 rows remain visible and inert with their missing integration. Duplicate provider IDs refuse overwrite, and setup-origin saves preserve the current/default pair. Provider-only edits preserve models/defaults and unrelated TOML. Confirmed deletion removes exact provider-scoped profile/context/review entries, deletes only the provider's exact Keychain reference, restores raw config on cleanup failure, and deterministically persists a replacement model when the deleted provider was current/default.
- `/provider` additionally owns four always-visible connection rows: Xiaomi MiMo, OpenAI Codex (ChatGPT), DeepSeek, and OpenAI API. Enter authenticates a disconnected row, connected rows open models, and Shift+L replaces authentication. MiMo and Codex select browser or headless provider-supported flows by platform; DeepSeek and direct OpenAI use masked API-key entry. A candidate is probed exactly once before automatic starter-model selection: 401/403 restores the previous credential/config, while another safe failure retains the candidate without switching.
- `provider_connect::resolve_runtime_provider` is the shared outbound boundary for normal TUI/headless requests, explicit doctor checks, provider probes, model refresh, and review construction. It alone injects refreshed Codex subscription credentials, preventing diagnostics and runtime requests from disagreeing about authentication.
- Fallback rules are profile-scoped raw config data and contain identifiers plus a closed cause list only. Config loading rejects missing exact models, duplicate primary pairs/causes, self-routes, empty causes, and unknown fields before secret resolution or traffic. Provider deletion removes only referencing rules; an exact model rename rewrites both primary and backup references while preserving unrelated TOML/comments.
- Model-picker discovery is read-only and asynchronous: bounded per-provider `/models` results merge with configured and small documented fallbacks, exact public catalogs avoid unnecessary credentials, failures are isolated, and external IDs/status text are sanitized at the terminal boundary. The selected provider exposes classified live/fallback/unavailable status without displaying URLs, response bodies, or secret material.
- Local-provider discovery is a distinct authority boundary: it accepts no runtime provider configuration, derives only `http://127.0.0.1:<approved-port>/v1/models`, disables proxies and redirects, sends no credentials/cookies, streams responses through fixed limits, and retains candidates only in process memory. Persistence begins only if the user saves the existing Add Provider form.
- BTW request preparation uses `context::protocol_complete_prefix_len`: complete submitted user messages and resolved tool exchanges remain available, while the first unresolved assistant/legacy tool message and every later message are excluded. Live partial output is already outside canonical messages and is never copied into the side request.
- Every provider consumer validates the same typed stream through `AssistantTurnAssembler`; channel EOF or transport failure is never a synthetic successful finish.
- Provider HTTP failures retain only status and typed transport class. Response bodies, request URLs, headers, and resolved credentials do not cross into application state or output; credentialless requests omit Authorization and unresolved environment references fail before I/O.
- Session semantics remain V4 with stable UUIDs, defaulted pin state, and ordered queued drafts. Legacy V1-V4 monolithic JSON remains readable without mutation; the next successful save atomically publishes storage-v1 metadata at the same stable `.json` commit point plus one generation-scoped append-only JSONL history log. Exact-prefix saves append only after prefix-hash and physical-length validation; rewrites or uncommitted tails rotate generations.
- `/sessions` (`/resume` and `/continue` aliases) lists bundle metadata without decoding messages, initially loads at most 64 recent records on a blocking worker, and fetches older pages by committed offsets. Its title-first browser groups the projected rows by local calendar date, labels the tabs `Recent` and `Archived`, keeps IDs/workspaces searchable but secondary, and renames active or archived metadata through the same UUID-checked blocking management lane. Its Recent projection excludes the current UUID before count, filtering, navigation, and actions; Archived remains reachable. A process-only generation cursor rejects stale pages; prepending preserves the visible bottom-relative transcript anchor. Provider submission and every other complete-history consumer wait for exact full hydration, while incomplete state cannot be rewritten as a complete session.
- `src/session_recall.rs` owns ordinary cross-session recall. It lazily builds a process-only SQLite FTS5 index from every retained completed CA, Codex, Hermes, and MiMo/Xiao projection on a blocking worker; no learning-window or corpus cap defines the searchable universe. `session_search` and local `/recall` share bounded search/browse/read/scroll actions with opaque cursors, exact-after-deterministic-redaction text, stable source/session/message IDs, and visibly untrusted provenance. The index is never persisted, source failures are isolated, identical transcripts retain all source provenance, and foreign databases are opened query-only without WAL/SHM creation.
- A fresh unnamed persistent session gets an immediate deterministic local title before its first request. After the first provider response, `src/main.rs` may launch one isolated tool-free `SessionTitle` request against the current resolved route. The main turn never waits for it. Separate usage/request tracing and a process-only session-ID/generation guard ensure failure or lateness cannot alter the conversation or an explicit/resumed name. Ctrl+G performs the same bounded generation for one selected legacy active/archive session after a blocking full load, then writes through exact atomic rename; opening/searching the picker never generates names.
- Bundle pinning changes metadata only. Archive/restore transfer the exact UUID-validated metadata/log pair with rollback; delete and retention unlink only that pair and surface companion cleanup failures. Branches never rewrite their source session and start with an empty queue.
- Session V4 fallback provenance contains only the initiating user-message index, provider/model identifiers, and typed cause. The transcript derives its visible marker from that data; branches retain only entries whose initiating user message remains, and canonical messages/defaults are unchanged.
- Queue add/edit/delete/reorder/steer/drain transitions update the current session snapshot. A successful completion persists a queue pop before starting the next provider request; save failure restores that draft and stops before network I/O. Drafts restored from disk remain visibly queued and process-locally ineligible for automatic drain until explicit user action.
- Saved-session deletion validates one storage-key component, re-reads the expected UUID, and unlinks exactly one inactive legacy JSON file or exact active bundle pair. The TUI runs it on a blocking worker; descendants, obsolete generations, and historical parent metadata are never traversed or rewritten.
- Saved-session pinning atomically updates one UUID-validated active file without changing its recency. Active lists sort pinned sessions first, then by update time; startup `-c` still selects the most recently updated session rather than the first displayed row.
- Archive and restore validate one storage-key component plus the expected UUID, then use a no-overwrite same-filesystem hard-link/unlink transfer between the selected profile's active and archive roots. They transfer either one legacy file or the exact active metadata/log pair; session bytes, descendants, obsolete generations, and historical parent metadata remain unchanged, and the current session cannot be archived.
- Optional session retention is absent/disabled by default and never runs automatically. Age and count rules operate only on freshly listed active summaries after current/pinned exclusion; archived roots are never inputs. Confirmed batches preflight every exact storage-key/UUID/regular-file/current/pin invariant before unlinking, never traverse lineage, and report honest partial unlink failure instead of claiming atomic rollback.
- Session branches retain additional workspace roots; missing restored roots are omitted with a visible warning.
- Session skill references retain only ID, canonical path, and SHA-256; missing or changed bodies remain inactive until explicit reconfirmation.
- Worktree records live only under the active profile data root and contain a UUID, validated label, canonical source repository, derived owned path, branch, base commit, Git common directory, and timestamp. Every mutation revalidates containment plus Git identity; symlinked or mismatched state fails closed.
- Worktree attach is explicit and idle-only. It replaces the process/session primary `WorkspaceContext`, reloads workspace memory/project skills, and makes later workflow loads and child spawn requests use that exact root. Already-running children retain their immutable spawn-time workspace snapshot.
- Worktree cleanup requires a non-attached owned record, a fresh clean status with no staged/unstaged/untracked files, and Cancel-first confirmation. It runs plain `git worktree remove` without force, removes only the manifest record/directory, and always preserves the branch. No shutdown, retention, workflow, goal, or agent completion path cleans worktrees automatically.
- Multi-run creates each candidate through `WorktreeStore` from the same frozen base and never attaches, merges, commits, cleans, deletes, or changes the current/default model. Partial creation, failure, cancellation, and shutdown preserve every created worktree and branch.
- Multi-run route identity and progress are process-only and secret-free. Exact clients are resolved only inside the provider runner; missing routes fail before provider traffic. Candidate, judge, and fusion requests have distinct usage/request ownership, while local diffs and generated prose never enter those ledgers.
- Multi-run judgment and fusion require fresh non-stale bounded evidence. Each request has exactly one non-executable local report schema, selects only an existing candidate/source, and cannot apply filesystem changes; fusion is an integration proposal rather than a merge.
- Work mutations carry an expected revision and apply serially; stale revisions cannot partially update a plan. Only user orchestration changes plan approval state.
- Validation evidence references a recorded command, test, or review event. Full forks regenerate plan/task IDs with source links; historical forks visibly omit unavailable work state and running agents never inherit.
- Child agents receive bounded summary context and exact tool/path/command/root/network grants. Their registry, preparation, sandbox, persistent-deny, and job-ownership checks are the same boundaries used by the parent and cannot widen through background execution.
- `read_file` streams UTF-8 input into a bounded 1-based inclusive line window and reports total/range/omission/continuation facts. Generic clipping is UTF-8 safe and reports exact omitted bytes.
- Each job stream retains an independent 64-KiB total head/tail capture with exact omitted bytes. Model formatting shares a near-4-KiB budget across stdout and stderr; terminal status, not formatted text, owns success/failure/cancel classification, and foreground waits keep their originating owner.
- WebFetch accepts safe textual content types, consumes decoded chunks through 1 MiB under the configured timeout, drops HTTP bodies on failure, and reports when the remainder was not read.
- Agent approval waits pause only that stable child ID. Manager shutdown cancels all children and their owned jobs; persisted nonterminal summaries restore as interrupted and never restart.
- Runtime files outside the repository are not test fixtures.
- Tests use temporary roots and do not depend on user sessions, config, clipboard, or credentials.

### Planner/executor experiment

- `planner-executor-experiment` is a non-default compile-time feature whose separate `ca-reasoning-artifact-experiment` binary owns both the offline RND-095 artifact commands and the explicit RND-097 pilot commands. It is not reachable through normal `ca` CLI/TUI/headless/service execution. Only explicit `run`/`resume` may enter the isolated broker/tool/WSP path after immutable fixture, price, spend, route, and sandbox gates.
- `schema` prints the JSON Schema for `reasoning_artifact.v1`; `validate` applies strict bounded semantic validation, requires each affected path to be either an existing checkpointed path or an explicit must-not-exist create target, intersects operations with declared tool/path capabilities, and can verify selected dependency digests under an aggregate byte budget; `cost` reduces strict `planner_executor_cost_runs.v1` input into `planner_executor_cost_summary.v1` with exact decimal accounting and dollars-per-success left unavailable when there are no successes.
- Its portable artifact has no schema field for provider-signed/encrypted response items, hidden reasoning, credential headers, or transcript dumps, and unknown fields fail parsing. Recognizable credential screening is defense in depth, not a guarantee that arbitrary prose contains no secret; artifacts remain sensitive repository data.
- The intended live architecture makes planner identity, task identity, repository anchors, and execution policy harness-owned. The current offline validator accepts those artifact fields from its caller and checks their shape/consistency; with `--repo` it verifies only declared dependency-path digests, not Git HEAD, the full worktree, or the task-input digest. Matching an artifact never authorizes execution.
- Experiment outcomes and explicit price/usage fixtures remain separate from the immutable artifact, whose canonical serialization has a deterministic SHA-256 identity. Every costed request supplies a date-stamped price-card ID and billing tier plus five disjoint token counters; totals are exact arithmetic over those caller-declared rates, not independent provider invoices. RND-097 adds an isolated live planner/executor/router/recovery controller behind the feature and explicit paid command; it never transfers provider state between phases or enables production routing.

### Context

- Canonical messages are never replaced by compaction.
- Checkpoints cover only complete turns and provider requests use checkpoint plus post-checkpoint messages.
- Explicit `/compact here [N]` derives complete canonical user exchanges, refuses unresolved tool interactions, summarizes only the prefix, and retains the latest bounded suffix without changing its serialized messages. Automatic compaction never uses this boundary.
- System prompt assembly retains tagged instruction/skill components while preserving the exact provider-facing bytes. After final tool filtering, orchestration emits a process-only breakdown for instructions, skills, built-in tools, MCP tools, checkpoint, conversation, and attachments.
- Estimator selection follows the exact resolved official endpoint/protocol/model contract for OpenAI/Codex, MiMo, and DeepSeek; unofficial compatible routes remain generic. All current families use a network-free conservative local estimator that never falls below the former UTF-8-bytes/4 result, and no family is labelled exact until a certified local tokenizer exists. Prepared fallback attempts recompute every category for the fallback route.
- Before a request is prepared, `/context` derives a clearly labelled preview without starting MCP servers. The latest prepared breakdown supplies status and checkpoint thresholds.
- `/context` maps the same seven estimated categories into a display-only 5x20 grid with stable largest-remainder allocation; exact textual totals, occupancy, limits, and checkpoint coverage remain authoritative.
- `/context` also reports the current sitting's distinct incremental,
  subscription-allocation, and unknown-cost states; latest completed foreground
  generation speed; working directory/roots; synchronous non-activating LSP
  status; and bounded plan-task counts/titles. Opening it performs no provider,
  filesystem, task, or LSP activation.
- Transcript checkpoint markers are derived at render time from the persisted boundary; neither the marker nor the dashboard summary modifies canonical messages or session data.
- Models without configured context limits compact manually only; known windows default to an 80% threshold.
- `/context-limit [provider/model]` edits the exact model profile when one exists, otherwise the exact `model_context` table. Window and checkpoint fields inherit independently through exact then legacy settings; empty threshold falls back to the automatic 80% policy.
- Context-limit persistence runs on the blocking mutation worker, preserves unrelated TOML/comments and model capabilities, reloads config, and invalidates only the process request-estimate/checkpoint-attempt cache. The idle-only command therefore applies to subsequent turns, while bare mode remains view-only.

### Memory recall

- Validated user and canonical-workspace records remain separately scoped and stored in their existing insertion order. Prompt-facing recall is a process-only projection selected afresh from the latest user request for interactive, headless, side-question, and service turns.
- Each enabled scope keeps its independent 25 KiB/200-line cap. At most eight recently updated broadly applicable preferences/workflows or workspace conventions/decisions share a 4 KiB reserve; the rest is filled by positive deterministic local BM25 matches ordered by score, query coverage, recency, then UUID. Records are emitted whole or skipped.
- Empty or unrelated queries do not inject external facts. Selection performs no network I/O, model download, vector embedding, persistent indexing, storage rewrite, or logging of memory/query text. Approved in-process mutations still rebuild recall; external owner-file changes are not watched during an active session.

### Skills

- Discovery does not follow symlink files or directories, and canonical project skills remain inside the applicable workspace root.
- Provider prompts receive a bounded metadata catalog. The primary model may
  activate an exact relevant qualified ID without generic approval, after
  which the full digest-pinned body appears on the next turn under the shared
  12,000-token aggregate budget.
- Compatibility roots are opt-in, aliases are searchable but not activation
  substitutes, and host-side lexical relevance loading is absent. Explicit
  persistent Ask/Deny rules still override automatic session activation, which
  grants no authority to later tools or declared dependencies.
- Hermes-compatible platform/tool/toolset/fallback/environment/config metadata is normalized into typed availability. Incompatible skills are excluded from normal model/slash discovery but remain inspectable; missing non-secret configuration is setup-required rather than hidden. Values resolve workspace override, profile value, then declared default, and only the exact activated skill receives quoted resolved data. Credential-like declarations/values and missing environment credentials remain outside this settings path.
- `src/skill_bundles.rs` discovers profile and workspace YAML bundles plus read-only foreign bundles. Two to five members resolve atomically to exact skill IDs and full-tree digests for one prompt, share the existing aggregate skill budget, and cannot collide with built-ins, prompt commands, skills, or another bundle. Explicit local mutations are journaled and registry refresh is immediate.
- `src/learning_journey.rs` stores owner-only lifecycle metadata with exact locators, digests, run/journal links, and no prompt/transcript/tool payloads. `/learning` (`/journey`) renders this ledger plus clearly labelled legacy inventory. Deterministic maintenance suggestions are read-only and cannot mutate, delete, install, or execute assets.

### MCP transports

- Constructing the manager never starts a child; enabled servers start only for catalog, resource, call, or explicit status/reconnect demand.
- Executables and argument arrays never pass through a shell. Child environments are allowlisted, configured secrets are display-redacted, and every server receives a dedicated temporary directory.
- Tool/resource catalogs, schemas, stderr, and returned text are bounded and terminal-sanitized. Binary and unsupported content fail closed.
- MCP children run under workspace/sandbox/network policy, form isolated process groups, and are closed and killed on failure, reconnect, cancellation, or application shutdown.
- Omitted `transport` remains stdio. Opt-in `streamable_http` accepts HTTPS or literal-loopback HTTP only, requires the existing distinct network authority, never uses ambient proxies or redirects, and shares the same discovery/call/resource/approval namespace.
- HTTP URL credentials, query/fragment data, process fields, reserved/literal headers, and unresolved header references fail before I/O. Header values remain exact environment references until the request boundary; JSON responses and raw SSE events are capped at 1 MiB and HTTP bodies/challenges never enter diagnostics.
- Opt-in HTTP OAuth is owned by `src/mcp_oauth.rs`. Normal catalog discovery never launches authorization: a missing credential becomes a typed `AuthorizationRequired` state, while `/mcp connect`, `/mcp refresh`, and `/mcp disconnect` are explicit generation-bound background operations.
- Browser authorization binds a literal-loopback callback before PKCE construction and pins protected-resource metadata, issuer, authorization, token, and client-metadata origins. An advertised RFC 8628 device flow is optional and requires a configured public client ID. Tokens live only in the profile/server-scoped `ca.mcp-oauth` Keychain item; bare's store is process-only.
- OAuth HTTP uses the same no-proxy, no-redirect, bounded-body policy as MCP transport. Raw challenges, endpoints, response bodies, authorization parameters, client secrets, tokens, and credential lengths do not enter manager state, snapshots, sessions, diagnostics, or Debug output.
- Ordinary interactive and headless request preparation reads `active_catalog()` and never starts a stopped MCP server. The main-profile built-in `search_mcp_tools` is the only model-triggered discovery entry point; it is removed with MCP capability (including bare) and cannot widen project trust or distinct network grants.
- Search starts at most eight eligible servers with concurrency four, or one exact requested server, then atomically activates at most ten exact names. Only activated schemas enter the next central tool-resolution pass, so `/context` and `/request` account for the exact process-local MCP set rather than every configured catalog.
- Tool/resource list-change notifications refresh only the already-running affected server and invalidate its activations before another request. Reconnect, disconnect, close, and shutdown clear the same scoped state. Explicit `/mcp` management retains a full-catalog path because it is direct user intent.

### Working-tree review

- Git reads use explicit argument arrays and never shell interpolation.
- The review surface exposes untracked path metadata but never opens untracked contents automatically.
- Review actions are read-only: no stage, revert, delete, or rewrite operation belongs in this surface.
- Fresh-context model review constructs new provider messages from project instructions and bounded Git input only; canonical chat, checkpoints, work state, skills, MCP, and agents never cross that boundary.
- Model-review diffs are capped at 1 MiB between complete file sections. Omitted and untracked paths may be named, but untracked contents are never loaded automatically.
- The review provider can use only registered read-only file/Git helpers and the internal structured findings reporter; successful results persist as typed Session V4 review records and a concise follow-up exchange.
- Opt-in convergence requires exactly two distinct configured provider/model routes. Both receive byte-identical objective material in separate fresh contexts and run concurrently under one request/output/exact-cost budget; there is no same-route fallback and both must pass.
- Headless `--check-converge` performs one pair by default. Existing explicit `--check-repair` authority may run normal repairs followed by fresh pairs, capped at three rounds and six reviewer invocations. Interactive repairs remain ordinary user-authorized agent turns followed by another fresh `/review converge` pair.
- `src/acceptance.rs` owns private immutable `ca.acceptance.v1` packets. Packets contain request/revision digests, relative changed paths, valid AGT receipts, hashed host events, reviewer verdict metadata, usage, and labelled claims; raw prompts, reviewer text, tool output, provider payloads, credentials, and absolute paths are excluded. Any source/workspace/session revision drift makes the packet stale.

### Shell jobs

- `JobManager` owns child processes for the `ca` process lifetime and terminates active process groups on normal shutdown.
- Foreground/background describes conversation flow; neither mode may block TUI event polling.
- Durable tmux sessions are external user-owned resources, never `JobManager` records. `/term` operations use exact argument vectors; attach pauses crossterm polling and restores raw/alternate-screen state on every exit path. Normal `ca` shutdown invokes no tmux cleanup.
- Job stdout and stderr are independently bounded, terminal text is sanitized for display, and stdin/cancel operations require a stable `JobId`.
- Background `run_command` retains the same approval classification as foreground execution; job-control tools do not bypass approval policy.
- Every job has a run/session owner; list, await, stdin, kill, and shutdown operations cannot cross ownership.
- Required process execution uses the platform sandbox backend or fails closed. macOS permits scoped system reads and workspace/job-temp writes; network requires both an explicit request and a host/run grant.

### Sandbox settings

- TUI startup captures one immutable effective sandbox mode before constructing MCP, subagent, job, approval, and tool-execution paths. Every TUI process launch uses that captured value even if the saved config changes later.
- `/sandbox` and the Sandbox row in `/config` expose only typed saved/effective modes, backend readiness, sanitized writable roots, and truthful network-enforcement text.
- `required` is the only OS-enforced mode. `policy-only` and `off` execute processes directly and never claim that approval policy is an OS network boundary.
- Weaker next-launch modes require a Cancel-first confirmation. Config mutation runs on the blocking worker, preserves TOML comments/unrelated data, and never hot-reconstructs managers. Bare mode is view-only.

### Durable scheduling

- `src/schedule.rs` retains the byte-compatible V1 definition, store, worker, result, and delivery contracts. V1 reads never rewrite state; checked revisions and short claim locks replace wrapping arithmetic and worker-long locks.
- `src/schedule_v2.rs` owns strict V2 definitions, UTC occurrence identity, five-field IANA cron/DST policy, the separate `schedules-v2.json` manager index, immutable hash-chained run receipts, and owner-bound leases. Durable claims precede time advancement; execution runs without the definition lock; settlement compares the exact definition digest, run ID, and claim revision.
- Delay and interval anchors are frozen only at exact digest confirmation. Stored schedules start paused. Missed intervals either skip or produce one catch-up; ambiguous cron folds choose the earlier UTC instant once and gaps are skipped.
- Existing-workspace jobs require read-only tools plus the available required sandbox. Workflow, script-only, and other mutation-capable jobs resolve one exact Git commit, reject dirty/untracked or unsupported trees, create a temporary detached tracked source, and run only in a WSP disposable non-Git root.
- Script-only execution revalidates one absolute executable digest, uses exact argv with no shell, clears the environment, has no provider/model path, and suppresses delivery for empty stdout. Scheduled skills are absent unless exact SKL-098 instance ID/full-digest pins re-resolve at execution.
- Mutating WSPs produce a complete owner-only recovery archive. The archive is independently reconstructed and compared with WSP's final manifest before cleanup authority is exercised; failed proof retains the workspace and evidence.
- `history` and `doctor` inspect offline receipts. Dead-owner recovery proves the recorded PID is gone, marks unresolved execution interrupted without replay, preserves evidence, and removes only a matching terminal lease. Corrupt, live, reused, or mismatched ownership remains blocked.
- LaunchAgent preview is deterministic XML containing only a revalidated absolute `ca` binary, exact profile, `schedule tick --drain`, controlled HOME/log paths, `RunAtLoad=false`, and a bounded cadence. Install/uninstall require the exact preview digest and matching plist; automated coverage uses a fake `launchctl` only.

### Typed mailbox tools

- `src/email/` owns exactly six opt-in Himalaya v2.0.x operations: account/list/read, immutable local draft, exact send, and frozen threaded reply. Tool selection requires both enabled email configuration and an explicit exact tool name; bare, scheduled, plan, review, checkpoint, and subagent profiles cannot gain them.
- Himalaya runs by absolute revalidated executable and fixed argv, with a cleared minimal environment, bounded JSON/stdin/stdout/stderr, timeout, and process-group cleanup. ca stores no mailbox credentials or configuration contents.
- Drafts and staged attachments are owner-only, create-new, fsynced, digest-bound records. SMTP recipients are explicit envelope arguments, so Bcc never enters the RFC 5322 message headers.
- Outbound approval is process-local, expiring, and single-use, bound to account, complete canonical To/Cc/Bcc, draft and attachment digests, tool call, and session. Every post-send-start error becomes `DeliveryUnknown` and is never retried.
- Email bodies and addresses may exist only in the active provider/tool loop and typed interactive preview. Hooks, normal events, sessions, portable exports, checkpoints, request snapshots, and terminal in-memory history independently replace them with metadata or protocol-valid private markers.

### Dedicated draft-first email gateway

- `src/email/{gateway,imap,mime,launchd}.rs` adds a separate disabled-by-default dedicated-account gateway. It cannot reuse an EML-100 account ID, has an empty-by-default exact sender allowlist, and resolves one profile/account-scoped app password only through the `ca.email.gateway` Keychain boundary.
- The production transport supports only certificate-verified IMAPS or required STARTTLS. Its public gateway trait can examine/fetch by UID, append `\Draft`, search exact Message-ID, and perform TLS/auth doctor checks; there is no SMTP delivery method. SMTP exists only as a doctor TLS/auth check and never submits a message.
- Strict bounded MIME parsing prefers local plain text, converts HTML without remote fetches, and exposes attachment metadata but never attachment content. Self/no-reply/daemon/auto-submitted/bulk/list and non-allowlisted mail settles before provider traffic.
- `GatewayStore` isolates state by profile/account, holds a crash-safe cross-process lock, and stores a hash-chained UIDVALIDITY/UID journal plus immutable private recovery bytes. Bootstrap starts at current UIDNEXT without marking or replaying existing mail. APPEND ambiguity searches the deterministic Message-ID; trustworthy absence permits one later byte-identical retry, while multiple/unavailable/continued uncertainty stops for manual recovery.
- Every accepted message becomes one fresh `RequestKind::EmailGateway` provider request containing a fixed instruction, one delimited untrusted message, and zero tools. It creates no normal session, memory, hook, checkpoint, portable export, skill/MCP/subagent/browser/shell state, and request inspection retains only private markers.
- `ca email doctor|gateway|service` owns offline/config/connect checks, once/daemon polling with SIGTERM-safe completion and bounded backoff, and exact-confirmation secret-free LaunchAgent lifecycle. V2 `EmailDraft` schedule delivery requires a canonical confirmed account/recipient set and invokes only the host Drafts appender.

## Dependency Direction

Preferred direction:

```text
CLI/TUI adapters -> application state/orchestration -> provider + tools + persistence
```

Avoid new reverse dependencies from provider/tool modules into ratatui. The current single-crate design allows direct calls, but new work should keep rendering, policy, protocol, and side effects separable enough to test.

## Known Pressure Points

- `src/main.rs`, `src/agent/tools.rs`, and `src/tui/input.rs` are large and combine multiple concerns.
- Interactive and headless tool loops can drift.
- Some typed command actions still perform synchronous work in `main`; ASY-006 owns moving long work behind asynchronous services.
- Tests are concentrated in a few module-local units; visual and protocol integration coverage is sparse.

Do not refactor these areas merely for size. First add a characterization test, then extract only the boundary required by a task.

## Oracle Relationship

- `oracles/cursor-CLI-reverse/cursor-agent/`: primary evidence for Cursor-visible commands, labels, tool cards, approval states, and protocol clues. It is minified/patched and must be corroborated.
- `oracles/cursor-CLI-reverse/cursor-proxy/`: experiments about transport/protobuf behavior; not part of `ca` runtime architecture.
- `oracles/codex/codex-rs/tui/`: examples of deterministic ratatui snapshot testing and documented input state machines.
- `reference/`: older extracted snippets, useful as secondary evidence only.

See `PARITY.md` for evidence ranking and acceptable differences.
