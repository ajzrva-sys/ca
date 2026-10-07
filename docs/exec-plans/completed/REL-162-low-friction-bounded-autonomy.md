# REL-162 — Low-Friction Bounded Autonomy

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-23

## Goal

Make the default interactive AutoReview mode behave like a practical coding
harness: safe in-scope local work continues without repetitive prompts, while
network, external communication, privilege, explicit Ask/Deny policy, and
out-of-scope writes retain clear boundaries.

## Non-Goals

- No Yolo semantics, generic external communication, implicit network access,
  privileged execution widening, or weakening of persistent Ask/Deny rules.
- No provider traffic, credential access, runtime config/session mutation,
  installed-binary replacement, or oracle/reference changes.
- No attempt to repair model-authored Python logic errors; the harness must
  instead preserve enough reliable tool/provider progress for the model to
  observe and correct them.
- No Cursor parity claim. The direct user report and screenshots are the
  acceptance evidence for this product correction.

## Evidence and Current Behavior

- User screenshots from 2026-08-23 show roughly twenty approvals in five
  minutes, read-only child agents denied access to requested transcript paths,
  three free-form unknown named agents reaching runtime, a sandboxed Python
  analysis failing on `/tmp`, and a provider timeout after the failed work.
- `src/permission.rs::auto_review_safe_invocation` admits only typed read tools,
  a small closed shell-read grammar, and narrowly read-only asynchronous
  children. Workspace edits, local validation commands, and every owned-job
  lifecycle action prompt again.
- `src/agent/subagent.rs` gives named children an empty `path_scopes` vector;
  asynchronous children also get no path scope when `assigned_files` is
  omitted. The child executor then denies every path resource outside that
  empty set.
- `src/workspace.rs::WorkspaceContext::resolve` rejects all reads outside
  workspace roots. Anthropic's official Claude Code security documentation
  describes the common boundary requested by the user: reads may occur outside
  the working directory while writes remain project-confined. Cursor's
  official permission documentation likewise supports absolute read rules.
- `task.subagent_name` is free-form in the model schema and is checked only
  after dispatch. Unknown values therefore consume a tool turn and, under the
  current mode, an approval.
- Ordinary jobs receive a private writable `TMPDIR`, but the sandbox denies
  other `/tmp` paths. The model-facing command contract does not explain that
  boundary, so conventional temporary paths fail late.
- Provider calls allow only 20 seconds for headers and first output and 60
  seconds without progress. The screenshot records a terminal provider timeout
  after the approval/tool failures.

## Scope

Allowed files:

- `src/{workspace,permission,jobs,sandbox,main,headless,role_run}.rs`
- `src/agent/{registry,subagent,client,client_tests,tools}.rs`
- focused TUI render/input tests and affected snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, `PARITY.md`,
  and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, runtime configuration/sessions, credentials,
  installed binaries, Git history, and unrelated dirty changes

Runtime state affected: build output and disposable test directories only.

## Design and Invariants

- Default AutoReview automatically executes registered local built-ins after
  preparation. Persistent Deny/Ask, network, outbound communication,
  privilege, skill activation, browser control, and unknown/dynamic tools keep
  their current explicit boundaries. Strict `ask` mode remains unchanged.
- Typed reads may resolve any canonical existing local path. Writes and creates
  remain confined to writable workspace roots. Sandboxed shell jobs may read
  the host filesystem but may write only workspace roots and their private
  per-job temporary directory.
- Owned-job await/stdin/kill operations do not create new authority; owner
  checks remain mandatory at execution.
- Read-only child tools inherit the same typed-read behavior. Mutating child
  tools default to workspace-root path scopes when no narrower assigned paths
  are supplied; explicit assigned paths remain narrower.
- Named agents are validated against the effective catalog before approval and
  the model contract names the built-in choices. Unknown names generate no
  child and no approval.
- Commands are told to use `$TMPDIR`; the private per-job directory remains the
  preferred temporary root. Conventional `/tmp` writes work, while sandbox
  denials keep every `ca-privilege-*` handoff path unreadable and unwritable.
- Provider timeout increases apply only to absence-of-response/progress windows;
  cancellation, retry caps, no-retry-after-output, and typed failures remain
  unchanged.

## Milestones

### M1 — Failing screenshot regressions

- Characterize global typed reads with confined writes.
- Characterize AutoReview local execution and owned-job lifecycle behavior.
- Characterize child inherited read/write scopes and unknown-agent preflight.
- Characterize private-temp command guidance and provider timeout defaults.

### M2 — Permission and scope correction

- Broaden AutoReview only within the stated local boundaries.
- Permit canonical typed reads outside workspace roots and sandboxed host reads.
- Repair child path-scope defaults and early named-agent validation.
- Make job lifecycle tools approval-free after owner validation.

### M3 — Failure recovery correction

- Add `$TMPDIR` guidance without widening temporary writes.
- Increase initial/progress provider windows enough for long reasoning/tool
  continuations while preserving bounded cancellation and retries.
- Update deterministic TUI text/snapshots if approval wording changes.

## Acceptance Criteria

- A default AutoReview turn can read an explicitly requested absolute path,
  edit within the workspace, run a local no-network validation command, and
  await/control its owned job without an approval prompt.
- The same operations still prompt under strict `ask`; persistent Ask/Deny
  wins in every mode.
- Network, outbound email, privilege, skill activation, browser control, MCP
  mutation, and unknown tools remain explicitly gated or rejected.
- Reads outside the workspace succeed, while direct and sandboxed writes
  outside workspace or temporary roots fail.
- Safe child agents can read requested external paths; mutating children can
  write only their assigned paths or inherited workspace roots.
- Unknown named agents fail before approval and the model sees the valid names.
- Command schemas tell the model to use `$TMPDIR`; conventional `/tmp` remains
  compatible without exposing private privilege-handoff prefixes.
- Provider calls tolerate 60 seconds to headers/first output and 120 seconds
  without progress, with existing retry/cancellation invariants intact.

## Validation

Focused:

- permission, workspace, sandbox/jobs, registry, subagent, provider-policy,
  main orchestration, and affected render tests

Completion:

- `cargo test --workspace --no-fail-fast`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo build --workspace --release`
- release help/version, diff/snapshot hygiene, and protected-tree checks

Manual/visual:

- Compare deterministic output with all three supplied screenshots. A native
  provider run is not required and must not be claimed unless separately run.

Blocked checks:

- No live provider or installed-binary acceptance without separate authority.

## Progress

- [x] Correlated the screenshots with current approval, workspace, child,
  sandbox, and provider timeout code.
- [x] Add failing focused regressions.
- [x] Implement permission/scope/preflight/temp/timeout corrections.
- [x] Complete focused and repository validation.
- [x] Reconcile harness documents and move this plan to completed.

## Decisions

- Decision: AutoReview means bounded local autonomy; strict `ask` remains the
  opt-in prompt-every-action mode.
  Evidence: the direct user report, current default configuration, and official
  harness documentation separating read/local work from external/destructive
  escalation.
  Consequence: registered local workspace work stops prompting by default, but
  explicit policy and external authority remain ceilings.
- Decision: prefer private job temp but preserve conventional `/tmp`
  compatibility.
  Evidence: the screenshot's literal `/tmp` failure is a common coding-harness
  workflow, while SEC-158 private privilege state also lives below host temp
  roots.
  Consequence: ordinary jobs may write `/tmp`, but late sandbox deny rules
  protect `ca-privilege-*` paths across macOS temp aliases.

## Surprises and Discoveries

- The child grant object records workspace root IDs but named children still
  receive an empty path scope, making ordinary read agents unusable.
- The retry after the model's Python `TypeError` reached a separate sandbox
  temp-path failure, then the provider timeout prevented another recovery turn.
- Broad interactive reads initially changed role-run's late swapped-symlink
  failure shape. Final-node no-follow validation now rejects that swap before
  ordinary canonical preparation while preserving its hardlink checks.

## Recovery and Rollback

Revert only this plan's localized source, test, snapshot, and harness hunks.
Do not reset or clean the existing dirty worktree.

## Completion Record

Changed files: `src/agent/{client,client_tests,registry,subagent,tools}.rs`,
`src/{headless,jobs,main,permission,role_run,sandbox,workspace}.rs`, `CONTINUITY.md`,
`NEXT_STEPS.md`, `TESTING.md`, and this plan.

Commands and results: focused screenshot regressions passed; the native macOS
sandbox external-read/write-confinement test passed; all 17 role-run CLI tests
passed; `cargo test --workspace --no-fail-fast -- --quiet` passed 2,472 tests
with three ignored; formatting, strict workspace/all-target Clippy, release
workspace build, release help/version smokes, snapshot/protected-tree hygiene,
and `git diff --check` passed.

Remaining risks: the screenshot's Python `TypeError` was model-authored and is
not a harness defect; the updated tool contract and reliable continuation make
it recoverable. No live-provider or installed-binary acceptance test ran. The
existing macOS debug-linker compact-unwind warning remains.

Harness updates: `CONTINUITY.md`, `NEXT_STEPS.md`, and `TESTING.md` record the
final behavior and evidence. No parity claim was added.
