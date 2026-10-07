# Supervised `ca` Integration

Status: INT-190 implementation is complete and hermetically validated. INT-191
adds the tracked protected-tree baseline and isolated named profile; INT-193
adds automatic bounded preflight/verification; INT-194 adds dependency-aware
selection, deterministic failure diagnosis, exact mutation batches, and saved
workflow recovery. The integration remains profile opt-in and default-off. No
live provider or service is required.

When the named CA profile is enabled, ordinary Main/headless work may obtain
bounded evidence before provider I/O and gate post-edit continuation on a
durable codex-team receipt. Focused verification follows owning packages and
bounded transitive reverse dependencies. A graph failure or excessive fan-out
may use a full suite only when the typed request explicitly carries profile
authority for that escalation. Failed evidence is classified without a rerun
before a repair continuation. Role-run, Plan, review, checkpoint, service,
scheduled-model, subagent, and bare surfaces still receive no delegation
facade or automatic continuation.

## Ownership model

`codex-team` remains the project governor. It owns task admission, role order,
scope snapshots, verification, evidence, acceptance, and commits. `ca` is an
optional execution engine for an individual role and an optional MCP cockpit;
it does not become a second scheduler while an orchestrated cycle is active.

The production profile is deliberately supervised:

1. Import one strict task object by previewing it first.
2. Apply that exact task with its preview digest and repository revision.
3. Run one cycle.
4. Inspect the resulting diff, guard and verification evidence, and pending
   commit intent.
5. Commit only through a separate digest- and revision-bound approval.

Do not use `run --auto-continue`, broad approval bypasses, nested agents, shell
tools, browser/network tools, or automatic disposition in this profile.

## Configuration

Legacy configuration remains Codex-backed and automatically committing. The
integration is opt-in:

```toml
commit_mode = "manual"
require_clean_start = true

[ca]
enabled = true
bin = "/absolute/path/to/ca"
profile = "codex-team"
route_provider = "openai"

[roles.worker]
provider = "ca"
model = "gpt-5.6-sol"
effort = "high"
timeout_seconds = 900
sandbox = "workspace-write"
approval_policy = "never"
web_search = "disabled"
```

The `sandbox`, `approval_policy`, and `web_search` role fields remain readable
for configuration compatibility, but they cannot widen `ca` authority. A
`ca`-backed role requires an enabled integration and a validated absolute
executable. There is no fallback to another role engine.

## Role process boundary

The orchestrator launches only:

```text
/absolute/path/to/ca role-run --profile NAME
```

It sends a `ca.role_run.v1` object on stdin. Advisory roles receive only fixed
repository-read tools. Worker receives those tools plus `apply_patch` and
`write_file`, with process grants derived from the task's normalized allowed
file patterns. Git, shell, delete, network, MCP, browser, jobs, subagents,
skills, memory, and external communication are never granted.

Worker grants fail closed on existing symlink prefixes/subtrees,
case-insensitive protected or runtime roots, and custom guard-ignore patterns.
The same scope is rechecked by the Rust snapshot guard after execution.

The child must emit bounded `ca.headless.v2` JSONL. Success requires one stable
run ID, monotonic sequence numbers, exactly one object-valued
`structured_output`, and a later `completion`. Approval requests, failures,
malformed output, timeouts, cancellation, or nonzero exit stop the cycle before
guarding, verification, staging, or commit. Rust's post-run scope guard remains
authoritative even when the process grants succeeded.

This boundary has orchestrator unit/fake-executable coverage and an
ignored-by-default real-binary compatibility test. With `CA_BIN` selecting the
clean target integration build, that test passed offline preflight, all four
orchestrator role schemas, one exact-grant Worker write, and one denied
outside-scope Worker write through `RealCaRunner`. It used an empty child
environment, disposable HOME/profile, loopback provider, test-only credential,
canonical temporary Git workspace, and contained output schemas. It verified
the outside canary remained unchanged, artifacts were sanitized, and no `ca`
data/session state was created. This is cross-process contract evidence, not a
live-provider pilot or permission to enable `provider = "ca"` in production.

## Task import

Preview does not mutate the repository:

```text
codex-team task-import --repo WORKTREE --input TASK.json --json
```

Apply requires the exact preview digest and current repository revision:

```text
codex-team task-import --repo WORKTREE --input TASK.json \
  --accept-digest DIGEST --expected-revision REVISION --json
```

V1 accepts exactly one `codex-team.task_import.v1` object. Unknown fields,
absolute or escaping paths, orchestrator runtime paths, writable protected
trees, dependencies, and network requirements are rejected. Apply writes only
orchestrator state and an immutable import receipt; it does not call a model,
edit project control documents, stage files, or commit. A durable transaction
record orders state and receipt publication. If execution stops after the
state save but before receipt publication, later work fails closed and an
exact replay repairs the missing immutable receipt.

## Typed `ca` verification

Imported verification commands are parsed without a shell and normalized to
these exact recipes:

```text
cargo test --no-fail-fast
cargo test --all-targets --all-features --no-fail-fast
cargo test --all-targets --all-features --no-fail-fast --quiet
cargo test SAFE_FILTER --no-fail-fast
cargo test SAFE_FILTER --no-fail-fast -- --nocapture
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --all-features
target/release/ca --help
target/release/ca --version
git diff --check
codex-team verify snapshot-hygiene
codex-team verify protected-trees
```

Environment prefixes, manifest/target overrides, arbitrary packages or
binaries, lint weakening, pipes, redirection, command chaining, expansion, and
network programs are rejected. Snapshot and protected-tree checks execute as
Rust-owned built-ins rather than shell commands.

## Manual commit approval

A successful manual-mode cycle pauses with a fingerprinted pending intent. HEAD
and the index remain unchanged. After reviewing the exact paths and evidence:

```text
codex-team commit-pending --repo WORKTREE --id PENDING_ID \
  --expected-digest DIGEST --expected-revision REVISION --json
```

The repository revision binds HEAD, bounded Git metadata, staged blob
identities and modes, and the bytes/types/modes of every dirty worktree path.
Any change to those inputs, the pending path set, or its fingerprints rejects
approval without clearing the intent or discarding Worker files. Commit
application uses literal pathspecs, disables hooks and signing, verifies the
staged and resulting commit path/content/mode set, and records durable recovery
state before the Git boundary. An exact retry can reconcile the narrow case
where Git succeeded but clearing the pending state failed. Creating a pending
intent terminates the current cycle immediately. V1 intentionally has no
automated reset/discard command.

## MCP modes

The MCP server pins one canonical Git root at startup:

```text
codex-team mcp --repo WORKTREE --access observe
```

All modes advertise an empty MCP resource catalog. Observe exposes exactly 11
read-only tools: `status`, `doctor`, `repo_query`, `recipe_list`,
`recipe_preview`, `self_improve_status`, `self_improve_objectives`,
`self_improve_evaluations`, `self_improve_traces`,
`self_improve_deployments`, and `step_status`. `repo_query` accepts at most
eight revision-bound actions and returns at most 64 KiB of redacted output; it
reads only tracked regular files and never persists raw results.

Delegate is the observe catalog plus exactly `delegation_start`,
`delegation_status`, `delegation_cancel`, and `delegation_feedback`:

```text
codex-team mcp --repo WORKTREE --access delegate
```

Delegated evidence chains reuse the production no-shell verification registry
and isolation runner. When production verification isolation is unavailable,
delegate mode still starts but its readiness receipt disables evidence while
leaving pure queries and contained local observation available. Evidence and
observation use durable jobs. Cancellation may terminate only executor-owned
children or stop a watcher; it never signals an observed PID.

### CA first-turn delegation facade

Normal interactive and headless CA runs can opt into the delegate tier in one
active-profile config. The server must be enabled local stdio, declare
`network = false`, advertise zero resources, and expose the exact 15-tool
delegate catalog. Observe-only and operate-capable catalogs are rejected.

```toml
[mcp.servers.codex_team_delegate]
command = "/absolute/path/to/codex-team"
args = ["mcp", "--repo", "/absolute/path/to/worktree", "--access", "delegate"]
enabled = true
network = false
timeout_secs = 30

[mcp.mechanistic]
enabled = true
server = "codex_team_delegate"
max_wait_seconds = 900
automatic_preflight = true
automatic_verification = true
preflight_timeout_seconds = 10
full_suite = "release_or_unscoped"
max_repair_passes = 2
allow_unisolated_evidence = false
```

The table remains disabled by default. When it is enabled, omitted automation
fields receive the values shown above, so existing named profiles require no
byte change. Enabling this table adds one first-turn
`delegate_mechanistic` tool; it does not start the MCP process while preparing
a model request. Its model-visible contract automatically prefers codex-team
for eligible bounded repository reads, local observation, and verification
before ordinary exploration or spawning an agent; the user does not need to
name the tool. CA also owns the facade protocol version: the model supplies
only typed `actions`, while CA constructs the versioned downstream request.
The first actual delegation checks the exact catalog, obtains
the pinned repository revision through host-owned `status`, and routes pure
queries to `repo_query` or evidence/one observation to a durable job. CA polls
that job itself in bounded waits, so the provider sees one call and one final
receipt rather than a search/poll loop. The configured server is excluded from
model-driven MCP search.

Before the first ordinary Main/headless provider request, CA also recognizes a
small deterministic set of unambiguous symbol, feature/edit, status/diff,
history, approved-recipe, and fully specified local-observation intents. It
sends only extracted typed operands, runs at most four actions, and attaches a
redacted 16 KiB receipt as request-only context. Raw prompts and query outputs
do not enter mechanistic telemetry; successful query receipts are cached only
in process memory by repository revision and action digest.

After a successful edit batch, exact file-mutation receipts select
`verify_changed_files` focused checks. Unknown-scope mutations compare the
pinned revision before choosing full ecosystem verification, and
documentation-only changes avoid product suites. Provider continuation waits
for the durable terminal receipt. Failed, stale, denied, cancelled, and timed
out evidence remains authoritative; at most two repair continuations are
allowed and each subsequent edit is verified again. Explicit user suppression
produces a skipped/unverified receipt rather than a passing claim. Only
successful production-isolated evidence is authoritative or cacheable. The
explicit `allow_unisolated_evidence` escape hatch is advisory, cannot satisfy
success, and never claims network denial.

Schedule V2 may select one pure mechanistic query batch or evidence chain. A
due mechanistic schedule calls this same delegate server directly, creates no
provider request or session, and does not persist observation ownership.
Scheduled prompt/workflow runs still omit `delegate_mechanistic`. A bounded
process-local event coordinator deduplicates turn, edit, revision, delegation,
watcher, and due-schedule events; the coordinator itself never creates provider
traffic.

`/delegate [JOB_ID]` and `/jobs` provide host-owned readiness, recovery,
cancellation, and categorical feedback. These controls make no provider
request and do not alter token accounting. Plan, review, checkpoint,
scheduled, service, subagent, bare, and `role-run` surfaces omit the facade.
In particular, `ca role-run` stays MCP-free so codex-team can govern a CA-backed
Worker without recursive orchestration.

Operate is the strict superset and adds exactly 13 tools:
`task_import_preview`, `task_import_apply`, `step_start`, `step_cancel`,
`commit_pending`, `disposition`, `recipe_approve`, `recipe_reject`,
`recipe_reactivate`, `recipe_retire`, `self_improve_pause`,
`self_improve_resume`, and `self_improve_rollback`. It is explicit and is
allowed only with manual commits, clean-start enforcement, and cancellable
Codex/`ca` role engines. Long steps are asynchronous: start once, then poll the
returned job rather than retrying start.

No model-visible tool may override the pinned repository. A pending commit or
human pause must be resolved through a separate exact, human-approved tool
call. Server shutdown cancels active work and preserves partial-state evidence.
Cancelled, timed-out, failed, and interrupted jobs set a durable approval latch
that changes the repository revision, so an earlier approval cannot be replayed
after potentially partial work. Operate startup also detects a durable Running
record from a previous server, terminates a safely identified live orphan
process tree before marking it Interrupted, and fails closed if ownership or
termination cannot be proved.

Task/import/planner contracts may include typed `mechanistic_actions` and
`delegation_policy = legacy | prefer | require | deny`. Existing tasks default
to `legacy`. Query receipts may supply compact role context; only admitted,
machine-checkable `rust_only` evidence work can complete without Worker.
`prefer` records any fallback explicitly, `require` forbids AI fallback, and a
real verification failure never becomes a silent fallback.

Learned skills may contain one inert `DELEGATION.json`. Rust canonicalizes its
typed action digest and keeps it out of prompts. Packages without a valid
manifest remain prompt-only; stored scripts, command strings, binaries, and
executable members are never executed. Three successful shadow matches across
two repository revisions create an unapproved proposal. Activation requires
exact human approval of the manifest digest and current repository revision,
one unambiguous match, and no drift, quarantine, retirement, or human negative
evidence.

## Rollout gate

Automated fake-process coverage is not live-provider evidence. Before a pilot:

- pin the exact repository revision and current source-state digest;
- verify the committed protected-tree baseline and preserve the current source
  state without reset, stash, or cleanup;
- run doctor and observe-only MCP discovery first;
- run one task and one step only;
- inspect HEAD, index, dirty paths, protected trees, snapshot hygiene, and all
  evidence before approving a commit;
- do not merge, mutate the default profile, or reuse the pilot worktree without
  separate authorization.

Dirty Git state is recorded evidence, not a categorical local activation
failure. Local installation is instead bound to exact validated artifact
hashes, a passing protected-tree receipt, recoverable binary backups, and
post-install real-client checks. Publication policy may still require a clean
tracked source independently.

Ignored protected evidence is now checked against the manifest committed at
`HEAD` as `.codex-team-protected-trees.json`. Use `protected-baseline preview`
to obtain a double-scanned candidate and `protected-baseline apply` only after
exact human approval of its revision and digest. A worktree-only manifest edit
never grants trust, and codex-team never rebaselines automatically.

In addition to the checked-in SDK/fake-process tests, the explicitly invoked
release-binary test passes through the actual `ca::McpManager`: exact
observe/delegate/operate catalogs of 11/15/28 tools, zero resources, status,
one facade query, task preview/apply, stale rejection with zero mutation, and
one credentialless asynchronous no-work step. The serial workspace matrix,
strict lint, formatting, all-feature release build, and production Seatbelt
subcheck pass. This is real-client compatibility evidence, not a supervised
MCP dogfood sequence or live-provider result; live traffic was neither
authorized nor run.

The installed opt-in profile is
`/Users/aji/.config/ca/profiles/codex-team-delegate.toml` (mode `0600`). The
installed CA and codex-team hashes are
`e68c2bfb434c3c9ad936b12f3e523666cd1000e35a7bca9d2dabf418197df870` and
`841eff1f53dd98372d014e0f42416ec9f00503f30427d3bf856a2a5432cb881a`.
`/delegate` and `/jobs` passed through the real TUI without a provider request,
and shutdown reaped the lazy stdio child. CA grants the exact configured
installed executable read-only access in its mandatory sandbox; codex-team
returns a mutation-free repository-only status before orchestration state is
initialized.
