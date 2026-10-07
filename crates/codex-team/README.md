# codex-team

A local macOS-friendly orchestrator that coordinates four active AI roles plus a deterministic Rust support layer. Role calls default to the **`codex`** subprocess; roles may also set `provider = "ollama"` or `"mlx_lm"` so Rust talks to local OpenAI-compatible servers. The ORCH-CA-001 integration branches additionally contain an opt-in, restricted `provider = "ca"` process boundary and an SDK-backed MCP cockpit. The clean `ca` integration branch implements the public `role-run` contract, and hermetic real-binary role and real-`ca` MCP-client compatibility tests pass. The integration remains **pilot-only** pending the final repository matrices and supervised pilots; no live-provider pilot has been authorized or run. Hermes is the optional boss supervisor (see `docs/HERMES.md`): it may drive MLX directly for toolful edits, and it must **not** impersonate Rust roles via `delegate_task` for admit/verify/commit. The internal role keys `master`, `planner`, `foreman`, and `worker` stay stable; old `intern` config keys still deserialize as ignored legacy compatibility.

- **CEO** (`master`): highest-reasoning direction layer that owns mission, priorities, non-goals, and completion calls.
- **Project Manager** (`planner`): roadmap and ticket compiler that turns direction into ordered work.
- **Senior Dev** (`foreman`): technical guardrail and review layer that decides whether approved work is acceptable.
- **Worker** (`worker`): bounded coding executor that receives one approved task at a time.

CEO is the mission-support layer. It reads the current roadmap and queue,
keeps the north star explicit, names the next strategic gate, and gives Project Manager
enough guidance to keep compiling small Worker tasks without asking the human
for routine direction. It should not duplicate existing approved tasks or mark
the project complete while `NEXT_STEPS.md` or `NEXT_STEPS_non-ai.md` still has
unfinished work.

Project Manager is the project governor and task compiler. It must choose one path:
clarify planning only when execution is unsafe, generate a bounded planning
batch when the queue lacks executable slices, or approve 1-3 safe Worker tasks.
If no tasks are approved, Project Manager must either approve safe execution or explain
why execution remains unsafe.
Project Manager should put deterministic command-only verification/read-only
work in `NEXT_STEPS_non-ai.md` instead of spending Worker tokens.

Senior Dev does not invent work. It consumes already-approved work orders and
Rust-produced evidence, then returns a decision: accept, reject, request
Project Manager, or request CEO. Rust owns task status updates, deterministic
evidence notes, and commits after acceptance or typed role authorization. If a
task definition is bad, Senior Dev routes back to Project Manager instead of
improvising.

The Rust support layer replaces the former AI Intern role. It deterministically
builds task support briefs, queue snapshots, failure summaries, review-assist
packets, safe command receipts, and missing-file/risk warnings. These packets
are advisory context only; task cards, allowed files, forbidden actions,
verification commands, and stop conditions remain authoritative. Rust may
summarize `NEXT_STEPS.md` and `NEXT_STEPS_non-ai.md`, but it does not approve
roadmap direction, invent work, or repair source on its own.

The orchestrator itself shells out in narrow ways: it launches `codex exec` for
roles with `provider = "codex"` (default), POSTs to local Ollama or Apple
`mlx_lm.server` `/v1/chat/completions` when `provider` is `ollama`/`mlx_lm` and
the matching block is enabled, and the deterministic Rust lane may run
allowlisted command-only verification from Rust-owned recipes exported from
`NEXT_STEPS_non-ai.md` or mechanical tracking-doc tasks from `NEXT_STEPS.md`.
Legacy command strings are parsed into typed recipes first; shell control
operators, redirects, pipes, and unknown programs are rejected. Rust may
classify, record, and commit deterministic queue bookkeeping; it must not repair
code, edit source, invent tasks, or approve roadmap direction.

## Build

```bash
cargo build --release
```

Install the binary somewhere on your `PATH`:

```bash
mkdir -p ~/bin
cp target/release/codex-team ~/bin/codex-team
```

## Initialize Any Project

```bash
codex-team init --repo /path/to/your/project --mission "Ship the next milestone with small reviewed commits"
```

To correct the mission later without running CEO, PM, Senior Dev, or Worker:

```bash
codex-team mission --repo /path/to/your/project --set "Ship the current milestone"
```

This updates `.orchestrator/state.json` and, when the existing stored mission is
found in `ROADMAP.md`, rewrites only that mission text while preserving the rest
of the roadmap paragraph.

This creates:

```text
.orchestrator/
  config.toml
  state.json
  prompts/
  schemas/
  tasks/
  runs/
ROADMAP.md
NEXT_STEPS.md
AGENTS.md              # only created if missing
```

On a new or poorly documented project, the first CEO/Project Manager passes may also
create compact Markdown control docs when they are repo-derived and useful:
`CONTINUITY.md`, `TESTING.md`, `ARCHITECTURE.md`, and `Docs/.INDEX.md`
or `docs/.INDEX.md`.
Projects that already use an uppercase `Docs/` tree are adopted as-is for
control-doc routing. If a project has no root `ROADMAP.md` but does have
repo-specific direction in files such as `PARITY.md`, `CONTINUITY.md`, and
`NEXT_STEPS.md`, Project Manager should preserve those files as adoption truth
and create only minimal roadmap/pointer text when needed for the orchestrator
contract.
Conditional docs such as `TOOLS.md`, `ENVIRONMENT.md`, `DECISIONS.md`,
`KNOWN_ISSUES.md`, `SECURITY.md`, or `RELEASE.md` are created only when the
repository clearly needs them. The purpose is to let Worker operate from local,
strict instructions instead of repeatedly scanning the whole repo.

## Supervised `ca` integration (pilot only)

ORCH-CA-001 keeps `codex-team` authoritative for admission, role order, scope
guards, verification, evidence, state transitions, and commit mechanics. The
orchestrator-side implementation supports manual commit intents, digest-bound
task import, a restricted `CaRunner`, and observe/operate MCP modes. The sibling
workspace package at `/Users/aji/project/ca` implements `ca role-run`, its
offline preflight, and the Plan-styled recipe cockpit. Both packages now share
this monorepo, lockfile, and root build directory; historical integration
worktree tips remain available through the root repository's local archive
refs.

Initialize a disposable Git worktree for supervised behavior:

```bash
codex-team init --repo /path/to/worktree \
  --mission "One supervised integration task" \
  --commit-mode manual \
  --require-clean-start
```

Preview one strict task without mutation, then apply exactly the returned
digest and repository revision:

```bash
codex-team task-import --repo /path/to/worktree --input TASK.json --json
codex-team task-import --repo /path/to/worktree --input TASK.json \
  --accept-digest SHA256 --expected-revision SHA256 --json
```

Run one `step`, inspect `status --json --observe`, and approve a resulting
pending intent only through a separate exact call:

```bash
codex-team step --repo /path/to/worktree --json
codex-team status --repo /path/to/worktree --json --observe
codex-team commit-pending --repo /path/to/worktree \
  --id PENDING_ID --expected-digest SHA256 \
  --expected-revision SHA256 --json
```

Manual mode leaves HEAD and the index unchanged when it creates the pending
intent. A stale revision, digest, HEAD, dirty path set, file type, or content
fingerprint rejects approval. There is intentionally no reset/discard command.
Imported tasks live in orchestrator state and receipts; their completion does
not rewrite the importing project's Markdown queue.

## Controlled learned recipes (pilot only)

Successful typed evidence-only command chains can now become unapproved recipe
proposals. A human can inspect the exact commands, applicability, evidence,
limits, risks, and isolation health through `codex-team recipe` or the
host-rendered `ca` Plan cockpit. Approval is bound to the exact proposal digest
and current repository revision, performs a fresh isolated validation, and
publishes one immutable revision in the private machine-local catalog.

Approved recipes activate automatically only for one admitted evidence-only
task in an exact matching repository. Drift deactivates without blame; a real
command failure quarantines locally; failures in two distinct repository
fingerprints retire that revision globally. Recipe execution never edits
product files, uses the network, runs in the background, falls back to AI, or
synchronizes across machines. Approval and execution fail closed unless the
production verification-isolation capability is healthy.

The MCP server pins one canonical Git root and exposes zero resources. Observe
is the default. Its 11 read-only tools include `repo_query`, which batches up to
eight revision-bound tracked-file/search/list/status/diff/history operations
and returns at most 64 KiB of redacted output without persisting raw results:

```bash
codex-team mcp --repo /path/to/worktree --access observe
```

Delegate adds durable typed evidence and contained local observation without
granting orchestration mutations:

```bash
codex-team mcp --repo /path/to/worktree --access delegate
```

Its four additional tools are `delegation_start`, `delegation_status`,
`delegation_cancel`, and `delegation_feedback`. Evidence reuses the same
no-shell verification registry as internal routing and remains disabled if the
production isolation probe fails; queries and local observation remain
available. Cancellation stops only executor-owned work or the watcher, never
an observed PID.

Operate is the strict superset. It adds task import, orchestration steps,
pending-commit/disposition authority, learned-recipe decisions, and guarded
self-improvement controls. It requires
`commit_mode = "manual"`, `require_clean_start = true`, and only cancellable
Codex/`ca` role engines. A step is a durable asynchronous job: start once, then
poll `step_status`; never retry `step_start` while a job is active. This surface
has passing fake-process, process-tree cancellation, generic `rmcp` stdio, and
actual `ca::McpManager` observe/operate discovery coverage. The hermetic
`RealCaRunner` test also passes every advisory-role schema plus one allowed and
one denied Worker write. Final repository-wide reruns and supervised CLI/MCP
dogfood remain pending; live-provider use remains unauthorized. See
[`docs/CA_INTEGRATION.md`](docs/CA_INTEGRATION.md) and [`PLAN.md`](PLAN.md).

Tasks can opt into the same contract with `mechanistic_actions` and
`delegation_policy = legacy | prefer | require | deny`. Read-only query receipts
become compact Worker/Senior context, while eligible `rust_only` evidence tasks
can finish without a Worker call. Learned skills may propose an inert
`DELEGATION.json`; scripts and executable members never gain authority, and
automatic use requires shadow evidence plus exact human approval of the typed
manifest digest and repository revision.

## Run one cycle

```bash
codex-team step --repo /path/to/your/project
```

## Run many cycles

```bash
codex-team run --repo /path/to/your/project --max-cycles 25
```

By default, `run` prompts after each job before continuing to Worker, Senior Dev,
Project Manager, or CEO work. Legacy auto-commit configurations without a
`ca`-backed role may use `--auto-continue`; manual commit mode and every
`ca`-backed configuration reject unattended continuation and require one
supervised job at a time. Use `--pause-on-escalation` when unattended legacy
runs should stop instead of escalating to CEO.

## Inspect state

```bash
codex-team status --repo /path/to/your/project
```

## Answer a human-disposition pause

When Rust pauses on dirty/quarantined work that needs a human policy choice,
record that choice without an interactive TTY:

```bash
codex-team disposition --repo /path/to/your/project --decision keep-paused
codex-team disposition --repo /path/to/your/project --decision preserve
codex-team disposition --repo /path/to/your/project --decision recheck
codex-team disposition --repo /path/to/your/project --decision gate
codex-team disposition --repo /path/to/your/project --decision keep-paused --refresh-report
```

`keep-paused` leaves the pause active. `preserve` records the dirty set as
human-owned context and stops without PM/CEO/Worker. `recheck` records that the
human handled the disposition externally and lets Project Manager re-check on a
later explicit `step`. `gate` asks Project Manager to draft or queue an explicit
safety gate on a later explicit `step`. `--refresh-report` can be used only
with `keep-paused`; it regenerates the dirty-adoption report for an active
`rust_dirty_adoption` pause without clearing the pause or running roles.

## Watch Scaffold

```bash
codex-team watch --repo /path/to/your/project --task TASK-123
codex-team watch --repo /path/to/your/project --self-heal
codex-team watch-loop --repo /path/to/your/project \
  --orchestrator-repo /Users/andrewzimmerman/project/codex_team_orchestrator \
  --max-cycles 3 --self-heal
```

Watch mode reads local telemetry and writes
`.orchestrator/observations/efficiency_watch.md` / `.json`. It flags oversized
role prompts, back-to-back Project Manager/Worker context bloat, repeated task
failures, stale observation-memory verification hints, and Worker-run
deterministic build/test/benchmark commands that should be Rust evidence jobs.
It also scans recent completed Worker command logs for repeated read/search
commands and records redundant exploration as route-regret telemetry. This is
post-run observation only; it does not hard-block live `codex exec`.
Watch reports also include derived status and advisory tool-health cooldowns
from `.orchestrator/observations/tool_health.jsonl`; cooldowns do not suppress
role calls by themselves.

`--self-heal` applies only Rust-owned efficiency mitigations in `.orchestrator`:
it can restore `planner_context = "compact"` after prompt-bloat alerts and
persist stale verification-hint suppressions under
`.orchestrator/observations/suppressed_verification_hints.json`. It can also
materialize mechanistic handoff candidates under
`.orchestrator/observations/mechanistic_handoffs.md` / `.json`; those records
identify safe Rust evidence-job candidates or unsupported recipe/router gaps.
Watch self-heal does not approve work, enqueue tasks, run roles, stage, commit,
revert, or edit product files.

`watch-loop` is the bounded automation lane for efficiency tuning. Each
iteration runs at most one non-interactive target `step`, writes the watcher
report, applies the same safe Rust self-heal when requested, and stops on human
disposition pauses instead of prompting. If alerts remain and
`--orchestrator-repo` is supplied, Rust writes an external efficiency feedback
packet under that repo's `.orchestrator/observations/external_efficiency_feedback/`,
marks Planner due there, and runs at most `--orchestrator-cycles` repair cycles
through the normal orchestrator gates. If the orchestrator repo has not been
initialized yet, `watch-loop` initializes only that repo side using
`--orchestrator-mission` or its default mission. The loop report is written to
`.orchestrator/observations/efficiency_loop.md` / `.json` in the target repo.
If the orchestrator-repair side is paused or has no runnable cycle,
`watch-loop` stops instead of continuing to spend target-repo cycles.

## Route Benchmark

```bash
codex-team route-bench --repo /path/to/your/project
codex-team route-bench --repo /path/to/your/project --json
```

`route-bench` scores built-in synthetic task profiles plus recent approved task
profiles against existing execution lanes: Rust evidence, Rust bookkeeping,
Worker, Senior Dev, Project Manager, and stop. It writes
`.orchestrator/observations/route_benchmark.md` / `.json` and does not call
Codex or execute verification commands. Route scorecards and positive route
learning are advisory inputs for future routing; live routing remains governed
by Rust policy and route-regret rules.

## Export Local Commands

When you want to run the task verification locally yourself instead of spending
model tokens, export command scripts from the approved `NEXT_STEPS.md` queue:

```bash
codex-team commands --repo /path/to/your/project
```

This writes `.orchestrator/manual-commands/*.sh` plus an index. The scripts set
repo-local Swift/Clang module cache paths and run the exact verification
commands from each parsed approved task. They do not update task status, run
models, or commit changes. Use `--task TASK-123` to export only one task.

## Preflight

```bash
codex-team doctor --repo /path/to/your/project
```

`doctor` checks Git tracking, project files, prompt/schema directories, and the
configured role engines. It checks Codex and its model list only when a role
uses Codex. When a role selects `ca`, it also checks the configured absolute
binary, `ca --version`, and `ca role-run --profile NAME --preflight`; this does
not inspect credential contents or make a provider request under the intended
`ca` contract.

## Config

Use `models` to inspect or update role model names, reasoning effort, and
timeouts without hand-editing TOML:

```bash
codex-team models --repo /path/to/your/project
codex-team models --repo /path/to/your/project --role worker --model gpt-5.4-mini --effort low
codex-team models --repo /path/to/your/project --role pm --model gpt-5.4 --effort medium --timeout-seconds 900
```

Role keys are `master`, `planner`, `foreman`, and `worker`; aliases include
`ceo`, `pm`, `project-manager`, `senior`, and `senior-dev`. You can still edit
`.orchestrator/config.toml` directly for lower-level knobs.

The default config uses separate model tiers:

- CEO role: `gpt-5.5` with extra-high reasoning for direction, priorities, and product judgment.
- Project Manager role: `gpt-5.4` with medium reasoning for roadmap sequencing and ticket compilation.
- Senior Dev role: `gpt-5.5` with high reasoning for technical constraints and decision-only review.
- Worker role: `gpt-5.4-mini` with medium reasoning for bounded coding execution.

The cost contract is intentional: CEO should be called rarely, Project Manager
should compress ambiguous work into deterministic task cards, Senior Dev should
create technical safety and review the result at medium cost, and Worker should
do the largest possible share of bounded coding. Rust-side parsing handles task
briefs, deterministic failure summaries, queue snapshots, and review-assist
packets before any model is called. The former AI Intern path has been removed
from active execution. When work can be simplified into exact files, exact
steps, exact checks, and exact stop rules, it belongs with Worker. CEO is reserved for
roadmap direction, strategic risk, phase changes, and true repeated-plan
failure.

Periodic CEO review is disabled by default:

```toml
master_every_completed_tasks = 0
```

CEO still runs when the roadmap is missing, Project Manager/Senior Dev explicitly
request strategic direction, or you set `master_every_completed_tasks` to a
positive cadence. Keeping it at `0` prevents routine Worker completions from
burning CEO tokens.

Project Manager is instructed to approve small ordered batches rather than
single-card drips when a readiness packet or prior task log already makes the
next local-only sequence clear. Returning zero work no longer escalates to CEO
by itself; CEO escalation must be an explicit strategic request.

`intern_preflight`, `intern_support`, `intern_read_only_commands`, and
`[roles.intern]` remain accepted in old config files, but they are ignored and
are not emitted by new initialization. Rust always generates support packets
deterministically.

`planner_context = "compact"` is the default token-saving path for Project
Manager. Rust precomputes a queue brief and gives Project Manager shortened
control-doc excerpts, with instructions to avoid source/test inspection unless
the control docs are insufficient. Set `planner_context = "full"` when you want
Project Manager to do a broad planning/reconciliation pass.

Display labels are configurable without changing state files or role behavior:

```toml
[role_labels]
master = "CEO"
planner = "Project Manager"
foreman = "Senior Dev"
worker = "Worker"
```

The default config intentionally avoids `--dangerously-bypass-approvals-and-sandbox`. Use a normal `workspace-write` sandbox first. If you choose full bypass, do it only inside an externally hardened VM/container.

`codex-team` automatically passes repo-local Swift/Clang module cache paths to
each `codex exec` role subprocess:

```text
.build/codex-team-caches/clang-module-cache
.build/codex-team-caches/swift-module-cache
```

This avoids macOS SwiftPM manifest-evaluation failures where sandboxed role
commands try to write under the user-level Clang module cache. The orchestrator
does not rewrite `HOME`, so Codex authentication and user config still come from
the normal account location.

`require_git = true` is enabled by default. Rust invokes `git` for deterministic
status checks, queue bookkeeping commits, accepted Worker commits, and recipe /
evidence hygiene. Senior Dev reviews the work; Rust performs accepted-task
state transitions and commit mechanics. In legacy auto mode Rust commits; in
manual mode it records an exact pending intent for a later human-approved
`commit-pending` call. Project Manager and Senior Dev may authorize
typed Rust commit requests, but they never run `git add` or `git commit`
directly.

## Worker guard

Before and after every Worker task, the orchestrator snapshots the repository itself and compares changed files against the task card's `allowed_files` globs. Generated/cache directories such as `.git`, `.orchestrator`, `target`, `.build`, `.swiftpm`, `node_modules`, `.venv`, `.pytest_cache`, `.next`, `coverage`, `dist`, `build`, and `out` are ignored by default.

If Worker touches files outside its allowed set, the guard report is passed to
Senior Dev review as authoritative context. Senior Dev must reject or route back
to Project Manager for a corrected work order; Rust will not commit a guard
violation.

## Existing project docs

`init` is adoption-safe. If `ROADMAP.md`, `NEXT_STEPS.md`, or `AGENTS.md` already exist, they are preserved and the new `.orchestrator/state.json` starts with `needs_reconciliation = true` instead of forcing a CEO rewrite. Existing `PARITY.md`, `CONTINUITY.md`, `TESTING.md`, `ARCHITECTURE.md`, `Docs/`, and `docs/` control material is treated as adoption context, not as disposable scaffolding.

On the first `step`, Project Manager runs first in reconcile/adopt mode. It should preserve existing task IDs, status history, structure, and constraints, then queue the first safe approved Worker task. CEO only runs if Project Manager explicitly escalates or the roadmap is missing/stale.

If the repo has little or no documentation, CEO is allowed to run a
breadth-first read-only survey and generate the core control docs before
implementation work begins. Project Manager may maintain those docs when doing so makes
Worker tasks smaller, safer, and cheaper; Senior Dev limits itself to task status,
review notes, and narrow hygiene notes.

If you already initialized with an older version and want to force this behavior, edit `.orchestrator/state.json`:

```json
{
  "master_due": false,
  "needs_reconciliation": true
}
```

Do not replace the whole file with that snippet; just set those two fields.

## Running Until NEXT_STEPS Is Done

Use `run` when you want the team to keep working without manually calling `step`:

```bash
codex-team run --repo .
```

`run` loops through planning, Worker execution, and Senior Dev review. After each
cycle it prompts before starting the next job:

```text
[Team] Continue to next job? [Enter/y] one more, [a] auto-continue, [n] stop:
```

Press Enter/`y` to run one more job or `n` to pause. In legacy auto-commit mode
without a `ca`-backed role, `a` and `--auto-continue` run through the remaining
available work for that invocation:

```bash
codex-team run --repo . --auto-continue
```

Manual commit mode and any configuration that delegates a role to `ca` reject
both forms of auto-continue so each cycle remains explicitly supervised.

`run` stops when one of these happens:

- `.orchestrator/state.json` marks the project complete and both `NEXT_STEPS.md` / `NEXT_STEPS_non-ai.md` have no unfinished task sections.
- `max_cycles` is reached.
- A model asks for approval or hits a blocker that cannot be resolved by the configured CEO/Project Manager/Senior Dev loop.

`Done.`, `Rejected.`, `Superseded.`, and `Not planned.` are terminal. Live `Approved.`, `Pending approval.`, `In progress.`, `Started.`, and `Active.` sections are unfinished, so a stale `project_complete = true` state will reopen and let Project Manager/CEO reconcile the queue instead of stopping early. Historical `Blocked.` sections do not reopen old work unless they are listed in the active approved task queue.

An empty approved queue is not terminal while `project_complete = false`.
Project Manager gets a chance to approve or compile the next safe Worker task;
if Project Manager produces no work while the mission is still open, the run
escalates to CEO for direction instead of silently stopping.

When the in-memory `.orchestrator/state.json` queue is empty, the orchestrator also parses approved task sections from `NEXT_STEPS.md` directly. Sections with `Status: Approved.` and the standard task fields (`Task ID`, `Allowed files`, `Forbidden files/actions`, `Implementation`, `Verification commands`, `Definition of done`, plus optional `Read-only files`, `Dependencies`, `Discovery budget`, and `Stop conditions`) are converted into Worker task cards before asking Project Manager to compile new work. Task IDs can use your project’s naming scheme, such as `TASK-001`, `WEB-004`, or `API-124`. If no approved task can be parsed, Project Manager plans or escalates to CEO.

Worker is intentionally queue-shaped but not autonomous: the orchestrator
selects the next approved task, Worker executes only that task, then Senior Dev
reviews and Rust either commits or pauses with a manual intent before the next
task starts. Worker is instructed to
classify a dirty worktree instead of stopping reflexively. Pre-existing
tracking docs such as `NEXT_STEPS.md`, `CONTINUITY.md`, `TASK.md`, `PLAN.md`,
`ROADMAP.md`, `Docs/ROADMAP.md`, and `docs/ROADMAP.md` are not blockers unless they conflict with
the selected task.

## Live output

This version keeps Codex's raw JSONL stdout/stderr in `.orchestrator/runs/`, but the terminal gets a compact human renderer:

```text
Worker · gpt-5.4-mini · low · prompt ~2.3k tokens · raw .orchestrator/runs/20260427T150000Z-worker.stdout.jsonl

TASK-024 is the next approved task. I’m tightening the CLI help text without changing runtime behavior.

Read README.md
Searched for usage examples
Edited 2 files: README.md, src/cli.rs
Ran cargo test cli_help for 3s

TASK-024 -> done

Updated the help text and kept the command surface unchanged.
```

`config.toml` supports:

```toml
live_output = true
live_output_style = "codex"
live_output_preview_lines = 8
live_output_preview_chars = 1200
live_output_verbose_success = false
live_output_color = true
deterministic_tasks = true
automatic_recipe_harvest = true
recipe_harvest_recent_logs = 24

[ollama]
enabled = false
base_url = "http://127.0.0.1:11434"

[mlx_lm]
enabled = false
base_url = "http://127.0.0.1:8080"

[ca]
enabled = false
bin = "/absolute/path/to/ca"
profile = "codex-team"
route_provider = "openai"

[roles.worker]
provider = "codex" # or "ollama" / "mlx_lm" / "ca" (matching integration must be enabled)
model = "gpt-5.4-mini"
effort = "medium"
timeout_seconds = 900
```

When `provider = "ollama"` or `"mlx_lm"`, Rust POSTs to `{base_url}/v1/chat/completions` and ignores Codex-only knobs (`sandbox`, `approval_policy`, `web_search`, `effort`). There is no silent fallback to Codex if the local server is disabled or unreachable. For MLX, start `mlx_lm.server --model <hf-or-path>` first (default port 8080).

When `provider = "ca"`, Rust launches only the configured absolute binary as
`ca role-run --profile NAME`, sends one bounded `ca.role_run.v1` object on
stdin, and requires bounded `ca.headless.v2` JSONL on stdout. Advisory roles
receive fixed read tools. Worker additionally receives `apply_patch` and
`write_file` grants derived from the task's allowed paths; Git, shell, delete,
network, MCP, browser, jobs, subagents, skills, memory, and external
communication are not granted. There is no provider fallback. A compatible
clean `ca` integration build and the hermetic real-process contract tests now
exist, but production enablement remains gated on final repository validation
and a separately approved supervised pilot.

The default `live_output_style = "codex"` renders narrative summaries and dim activity lines like `Read README.md`, `Searched for ...`, `Edited 3 files`, and `Ran cargo test ...`. Use `live_output_style = "boxed"` to restore the earlier framed terminal layout. If you prefer quiet/background behavior, set `live_output = false`; if you want every successful command previewed, set `live_output_verbose_success = true`. Set `live_output_color = false` or export `NO_COLOR=1` for plain monochrome output.

For AI-backed implementation tasks, Worker should avoid spending model context
on the full final verification list. After Worker returns, Rust runs the task's
deterministic-safe verification commands and passes the result to Senior Dev.
Senior Dev should use that Rust verification evidence and only rerun checks
when Rust skipped/failed a command or when task judgment requires extra proof.
Worker can also return `handoff_to_rust` with final deterministic commands it
knows should be run. These are structured command objects, not shell strings:

```json
{
  "id": "app-smoke-tests",
  "program": "swift",
  "args": ["test", "--filter", "AppSmokeTests"],
  "cwd": null,
  "timeout_seconds": 600,
  "kind": "test",
  "failure_policy": "stop",
  "expected_exit": null,
  "tags": ["swift", "test"],
  "source": "worker.handoff_to_rust"
}
```

Rust first parses legacy task-card command strings into recipes such as
`CargoCheck`, `CargoTestNamed`, `SwiftBuild`, `SwiftTest`, `GitDiffCheck`,
`Ripgrep`, or `LocalSmoke`; then it expands those recipes into typed commands.
Worker handoff commands are already typed. Rust merges the two sources,
de-duplicates them by `program + args`, rejects unsafe commands, runs the safe
set without a shell, and includes both results and rejected commands in
`rust_verification`. This is the intended path for long chains of focused
tests, builds, and `.build/debug/<binary> --local-*` smoke commands.

Rust writes every verification run as append-only evidence under
`.orchestrator/evidence/YYYYMMDD.jsonl` and per-task command evidence under
`.orchestrator/evidence/<task-id>.jsonl`. Each report records the typed
verification plan, rejected commands, per-command duration, exit code,
classification, and capped output tails. Recipe runs also create durable
`.orchestrator/runs/<run-id>.json` records, while orchestration transitions are
appended to `.orchestrator/events.jsonl`. Rust also has scaffolding for cargo
JSON diagnostic parsing, conservative problem matchers, diff-filtered findings,
affected-work hints, action-cache keys, task policy validation, and repo
indexing so future prompts can receive findings and evidence instead of raw
logs.

The typed lanes now include the next batch of Rust-first review machinery:
supply-chain recipes for Cargo manifests, snapshot records with redactions for
unstable fields, staged/temp-worktree verification modes, robustness and fuzz
repro records, coverage/mutation quality recipes, budget findings, sandbox
profiles, bors-style batch/bisect records, feature/UI/release recipes,
annotations, AI trajectories, DICE-style compute keys, hook manifests, and a
ranked repo map. Most of these are scaffolds, but they are deliberately typed:
the goal is for Rust to inspect reality and hand AI compact receipts instead of
asking models to rediscover command output, repo shape, or policy state.

For low-risk tasks whose task card allows it, Rust may auto-accept after Worker
reports `done` when the guard is clean, all evidence passed, no handoff commands
were rejected, and the diff is inside the allowed files. Source edits stay
conservative by default: broad diffs, dependency or lockfile changes, generated
churn, security/auth/crypto terms, public/network behavior, failed verification,
or guard violations route to Senior Dev or Project Manager instead.

After Rust accepts a Worker task, either by auto-accept or by applying a Senior
Dev acceptance decision, Rust marks only the selected task `Done.`, inserts
deterministic evidence notes, stages allowed files plus tracking docs, commits
the accepted change, and captures the deterministic part as a recipe candidate
under `.orchestrator/recipes/`. Recipe candidates are advisory: Project Manager
may reuse them to export future command-only work into `NEXT_STEPS_non-ai.md`,
but they never authorize Rust to edit source, approve roadmap work, or invent
follow-up tasks.

Model-call telemetry is appended to `.orchestrator/cost.jsonl`. Each line
records the role, task id when known, configured model/effort, prompt token
estimate, output size, and whether Rust skipped that model call through
deterministic routing or auto-accept. This is intentionally boring accounting:
it gives you a local ledger for where tokens are still being spent. The `cost`
report also summarizes route-regret memory, including redundant Worker
exploration and estimated savings grouped by the suggested better Rust lane.
Tool-health summaries are written under `.orchestrator/observations/` and are
shown in `status`, `watch`, and `cost` as advisory cooldown evidence.

Project Manager responses also include a compact `work_plan` with goals, likely
task sequence, test strategy, rough time estimates, and triggers for returning
to PM. Rust persists the latest plan under
`.orchestrator/observations/planner_work_plan.md` and `.json`. The intent is
for PM to spend one planning call on several steps, then let Rust, Senior Dev,
and Worker consume the queue and evidence receipts without bouncing back to PM
after every small cycle.

Worker prompts also produce section-level prompt receipts under
`.orchestrator/observations/prompt_receipts/`. A receipt records the estimated
tokens and bytes for task text, role rules, repo hints, file contents, logs,
diff, previous attempts, and guidelines, plus the hard max budget and whether
Rust rejected the prompt before launching Codex. Retries use compact Attempt
Capsules by default, so repeated failures can send the fingerprint, changed
files, compact diagnostic summary, and next instruction instead of full old
transcripts, full logs, or full diffs. Before Worker launch, Rust prints the
context-pack token estimate, a rough context-review time, and the receipt path.

At the start of each cycle, Rust also harvests recent
`.orchestrator/runs/*.stdout.jsonl` logs for repeated successful deterministic
command chains. Harvested recipes are promoted automatically:
`candidate` after one sighting, `trusted` after two, and `auto-runnable` after
three. This harvest is advisory and never edits source or approves tasks; it
only gives Project Manager and Worker better command recipes to reuse.

Rust gives Worker compact task support briefs and gives Senior Dev
review-assist packets. It summarizes deterministic failures from command, exit
code, capped stdout/stderr, failed test names, top diagnostics, rejected unsafe
commands, and a route suggestion. Queue snapshots are produced from the
`NEXT_STEPS.md` and `NEXT_STEPS_non-ai.md` parsers; no AI role is launched for
this support work.

`deterministic_tasks = true` is also enabled by default. Before launching
Worker, the Rust orchestrator checks whether the selected approved task is a
clearly mechanical verification/read-only task whose editable surface is only
tracking docs such as `NEXT_STEPS.md`. If so, Rust runs the listed safe
verification commands itself, marks the task done in `NEXT_STEPS.md`, and
commits that bookkeeping without spending Worker or Senior Dev tokens. Any task
that needs source edits, architectural judgment, shell pipelines/control
operators, or broader file access falls back to the normal model path.

Task cards may optionally include routing hints. Missing fields remain
backwards-compatible and default to conservative AI review:

```text
- Review tier: `always_ai_review`
- Automation level: `ai_worker_ai_review`
```

Use `review_tier = auto_accept_if_verified` and
`automation_level = ai_worker_rust_review` only for small, low-risk work whose
acceptance is machine-checkable. Deterministic tracking-doc tasks use
`rust_only`.

Rust also evaluates simple conditional queue gates without Project Manager.
When `NEXT_STEPS.md` contains `Status: Conditional approved.` plus exact
`Run if:` predicates such as ``TASK-123.Outputs.flag == passed``, Rust reads
earlier `Outputs: flag=value` notes and queues the matching branch directly.
False conditional branches are marked `Skipped by gate.` with deterministic
bookkeeping and a commit. That keeps exact branch selection out of the model
loop.

The rough Worker decomposition is now:

- Rust: queue selection, conditional gate evaluation, dirty snapshotting,
  allowed-file guard, deterministic command execution/classification, false
  branch skipping, status-only bookkeeping, final verification, and recipe
  capture from accepted Worker tasks.
- Worker: source/test/doc synthesis when the task requires judgment or edits
  outside tracking-doc-only deterministic work; reports focused commands/tests
  so Rust can translate the repeatable part into future deterministic recipes.
- Senior Dev: decision-only technical review when Rust cannot safely
  auto-accept; rejects or routes back instead of committing or inventing work.

For tasks that should never spend model tokens, put the same rigid task-card
format in `NEXT_STEPS_non-ai.md`. `run` and `step` check that file before the
AI-backed queue. Approved tasks there must be deterministic verification or
read-only tasks with only tracking-doc edits allowed; Rust executes the listed
safe commands, marks the task done in `NEXT_STEPS_non-ai.md`, and commits that
bookkeeping automatically. If a `NEXT_STEPS_non-ai.md` task is not safe for
deterministic execution, the run stops rather than sending it to Worker.

`NEXT_STEPS_non-ai.md` may also contain a rigid `## Command Packet:` with
explicit `Edit Targets`, `Required Source Changes`, `Post-Edit Validation
Commands`, and `Stop Conditions`. Rust materializes that packet directly into
bounded Worker work, clears stale CEO/Project Manager/retry flags, and skips
CEO/Project Manager. The packet must name its edit targets explicitly; globbed
targets are expanded, but Rust does not infer project-specific files from
domain vocabulary. SwiftPM sandbox/cache failures inside Worker are treated as
known environment blockers; Rust still owns final verification. Packets without
enough structure stop before any AI role so the file cannot be reinterpreted
into broader work.

If a command packet contains a fenced `diff` or `patch` block, Rust treats it as
a true non-AI patch packet: it checks the unified diff with `git apply --check`,
applies it, runs the packet's deterministic validation commands, marks the
packet done in `NEXT_STEPS_non-ai.md`, and commits only the patched files plus
the queue update. Worker, Senior Dev, Project Manager, and CEO are skipped.

Project Manager is instructed to export command-only approved work from
`NEXT_STEPS.md` into `NEXT_STEPS_non-ai.md` automatically. If it does that and
returns no Worker tasks, the same `run` cycle immediately gives Rust a chance to
drain `NEXT_STEPS_non-ai.md` before pausing or escalating. The split is:
AI classifies and writes safe deterministic task cards; Rust executes,
classifies `passed` / `noMatches` / `knownEnvironmentBlocker` /
`commandFailed`, marks deterministic tasks done or known-blocked, and commits
only that bookkeeping. Rust does not repair code or invent follow-up tasks.
Unclassified deterministic failures remain unfinished and are summarized by
Rust before Project Manager or Senior Dev decides what to do next.
