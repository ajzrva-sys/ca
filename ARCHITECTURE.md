# Architecture

Module ownership for the current tree. The previous long map is archived at
[docs/archive/ARCHITECTURE-before-iso337-summary.md](docs/archive/ARCHITECTURE-before-iso337-summary.md).

## Source owners

| Area | Owner |
|---|---|
| CLI and turn loop | `src/main.rs` |
| Conversation and UI state | `src/app.rs`, `src/tui/` |
| Provider preparation | `src/agent/client.rs`, `src/agent/provider.rs`, `src/request.rs` |
| Exact native wire provenance | `src/host_instructions.rs`, `src/provider_replay.rs` |
| Task filter policy and local preview | `src/task_filter.rs`, `src/filter_review.rs`, `src/privacy/`, `src/tui/privacy/` |
| Tools | `src/agent/registry.rs`, `src/agent/tools.rs` |
| Child admission | `src/agent/subagent.rs`, `src/execution_grant.rs` |
| Jobs and environment | `src/jobs.rs` |
| OS sandbox | `src/sandbox.rs`, `crates/ca-automation/src/freebsd_sandbox.rs` |
| Destinations | `src/network_broker.rs` |
| Worker launch | `src/isolated_worker.rs` |
| Skills and learning | `src/skills.rs`, `src/skill_cli.rs`, `src/learning.rs`, `src/learning_journey.rs` |
| Checkpoints | `src/context.rs`, checkpoint functions in `src/main.rs` |
| Harness evaluation | `crates/ca-automation/src/self_improvement/harness.rs` |
| Confirmation | `src/harness_confirmation.rs` |
| Hint edits | `src/harness_hints.rs` |
| Security records | `src/security_journal.rs` |
| External effects | `crates/ca-automation/src/runtime.rs` (`RunStore`) |

`codex-team` re-exports the evaluator. It does not own a second one.

`ca-hrs-252/` is a side snapshot in this checkout. Do not edit it.

## Where to start in a large file

Function names below were read from this tree. Line numbers drift. Search the name if the line has moved.

| Change | Start here |
|---|---|
| Process entry and CLI parse | `main` (`src/main.rs` line 274), `parse_launch_args` (1488), `parse_cli_args` (1879) |
| TUI session | `run_tui` (`src/main.rs` line 4913) |
| One model turn | `forward_turn_attempt` (`src/main.rs` line 35274) |
| Filter stop during a turn | `enforce_provider_filter_stop` (`src/main.rs` line 35746) |
| Tool batch and dispatch | `execute_pending_tools_async` (39660), `execute_tool_dispatch` (39917) |
| Headless entry | `run_headless` (`src/main.rs` line 2906), then `src/headless.rs` |
| Key handling | `handle_event` (`src/tui/input.rs` line 169) |
| Transcript block keys | `handle_scrollback_key`, `fold_transcript_selection`, `open_transcript_selection`, `quote_selection` in `src/tui/input.rs` |
| Drawing | `render` (`src/tui/render.rs` line 578). Filter row: `paint_filter_status` (664) |
| Composer text and cursor | `normalize_boundary`, `previous_grapheme_boundary`, `next_grapheme_boundary` in `src/tui/composer.rs` |
| Pre-send filter review | `hold` and `dispatch` in `src/filter_review.rs` |
| Child tool preparation | `prepare_child_tool` (`src/agent/subagent.rs` line 2301) |
| Child spawn admission | `admit_request` (`src/agent/subagent.rs` line 5019) |
| Load config | `CaConfig::load` (`src/config.rs` line 2174), `ConfigStore::load` (2813) |
| Compile and arm the provider filter | `normalize` (525), `install` (4878), `publish_outbound_body` (5340) in `src/provider_filter.rs` |

Interactive root submissions enter `filter_review` before planning or sending.
The accepted task owns its filter snapshot and durable alias mappings; planner,
workers and tool continuations inherit that snapshot. Send authorization passes
through `WorkState` into the existing transactional graph-start path. Explicit
Plan review and restored paused work retain their existing review requirement.
Graph persistence and privacy receipt publication share the existing context
commit lock on a blocking worker. Changed graph state is durable before its UI
update; the terminal completion message is then appended once.

Filename discovery enumerates the root, then at most 25,000 descendant entries.
Denied descendants and bounded recursion yield process-only coverage notices;
an unreadable root remains an error. A partial census cannot establish bare-name
uniqueness, while exact encountered paths retain executable references and local
restoration. Coverage revisions invalidate previews and stay out of provider input.
The closed native structured-output contract binds the exact host-generated task
map before projecting descriptive strings; caller schemas and report values retain
their normal filtering and preflight checks.

Closed host instructions carry process-only spans through authored content and
adapter lowering. Exact wire receipts protect only those spans; dynamic, imported
and retrieved text remains filtered. The same exact-field mechanism retains
route/policy-matched provider reasoning continuation bytes. It does not make
reasoning executable or exempt visible text/tool arguments, and final restored
bytes still receive explicit stop checks.

Existing home-relative paths bind only when their canonical targets are within
current authorized roots. Local display retains the original spelling; native
execution rechecks the target. Native automatic task maps normalize typed path
fields to workspace-relative locations without rewriting task prose.

Literal issued-reference glob queries resolve only their target under the current
prepared base; wildcard queries retain normal enumeration. Failed listings remain
filtered diagnostics and do not establish listing roots. Duplicate-listing notes
explain target equivalence only when a live observation authenticates the marker
and its retained source result. URL masking preserves only authority spans issued
by that task; surrounding text and final stop checks still apply.

## Grants

`AdmittedExecutionGrant` is host state created at spawn. Model arguments can
narrow tools, paths, commands, and references. They cannot select
`child_execution` or widen authority. `isolation` is workspace placement
(`shared` or `worktree`), not the execution mode.

## Boundaries that stay

The fixed 100,000-token shared worker ceiling and separate planner accounting
stay in place. SessionV4 and Automation V2 stay. New security and confirmation
data is separate from those schemas. Provider authentication stays in the host.

## Known gaps

See [CONTINUITY.md](CONTINUITY.md). macOS absent child execution uses the
Seatbelt worker. Linux and FreeBSD do not. A restricted read holds the
admission lock until it finishes, so a concurrent unapproved send fails.
`RunStore::perform_once` does not replay an uncertain effect. A policy
generation change closes the live proxy and cancels owned jobs and workers.
