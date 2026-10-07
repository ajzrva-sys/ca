# INT-148 — Scoped `ca role-run` Protocol

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-21

## Goal

Add a public, versioned, process-scoped `ca role-run` contract that lets the
`codex-team` orchestrator execute one advisory or Worker role with exact local
authority. The command must support an offline preflight, accept one bounded
JSON request on stdin, emit only `ca.headless.v2` JSONL on stdout, enforce one
object-valued structured result, and terminate role-owned process groups and
reap direct children on cancellation.

The authoritative orchestrator design is now
`crates/codex-team/PLAN.md`. This plan records the target-side implementation
that began on `codex/ca-role-run-integration` from clean baseline
`abab2fbf3840283c0d0d09560d7dd2edd063a820` and was consolidated into the root
workspace on 2026-08-22.

## Non-Goals

- Do not expose or alias the private `schedule __worker` command.
- Do not add `--yolo`, persistent Allow, persistent Ask, or network authority.
- Do not enable sessions, memory, skills, plugins, MCP, LSP, hooks, subagents,
  workflows, jobs, browser, email, external communication, or model-visible
  network tools.
- Do not implement `ca serve` as the role backend.
- Do not modify real profiles, credentials, sessions, installed binaries,
  `oracles/`, or `reference/`.
- Do not send a live provider request without separate user authorization.

## Evidence and Current Behavior

- Clean snapshot baseline commit:
  `abab2fbf3840283c0d0d09560d7dd2edd063a820`.
- The original dirty checkout remains read-only at
  `855ca635a4d83a0a8464c06a6c750b16133494c7` with 574 status paths and status
  SHA-256 `3405eb39643ad8d17f6a4c8151939ea357e35904f47c038274b07bcb343a09ea`.
- `src/schedule.rs` already owns a bounded hidden `WorkerSpec`, process grants,
  and restricted scheduled execution. Its preparation mechanics are reusable,
  but its authority is broader than this public contract.
- `src/headless.rs` and `src/headless_contract.rs` already own
  `ca.headless.v2`, the sole-final structured-output reporter, event ordering,
  and output validation.
- `src/permission.rs` evaluates persistent Deny, process grants, Ask, and
  Allow. Role execution must retain Deny only as a ceiling and use request
  grants as its sole positive authority.
- `src/runtime.rs` owns capability restriction. A role-specific capability set
  must be built before any optional extension composition.
- `ConfigStore::for_process_only_load` may resolve credentials and perform a
  legacy approval-mode migration. It cannot be used by offline preflight.
- `run_headless` currently composes plugins before deriving scheduled
  capabilities. Role execution must skip plugin composition rather than load
  and filter it afterward.
- Baseline validation passed 1,651 default main tests, 37 CLI tests, 1,680
  all-feature main tests, 118 experiment tests, strict all-feature Clippy,
  all-feature release, CLI help/version, formatting, snapshot hygiene, diff
  hygiene, and protected-tree immutability.

Inferred: a separate `src/role_run.rs` boundary is safer than adding more
special cases to scheduled or ordinary headless execution. It can reuse typed
validators and the headless runner while preserving a closed public protocol.

## Scope

Allowed files:

- `src/role_run.rs`
- `src/main.rs`
- `src/config.rs`
- `src/runtime.rs`
- `src/request.rs`
- `src/schedule.rs`
- `src/headless.rs`
- `src/headless_contract.rs`
- `src/permission.rs`
- `src/provider_trace.rs`
- `src/agent/stream.rs`
- `src/agent/tools.rs`
- `src/mcp.rs`
- `tests/cli.rs`
- `tests/role_run_cli.rs`
- `.gitignore`
- `docs/ROLE_RUN_PROTOCOL.md`
- `docs/exec-plans/active/INT-148-role-run-protocol.md`
- `NEXT_STEPS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`,
  `docs/.INDEX.md`

Additional source/test files require an explicit discovery note in this plan
before editing.

Discovery note: `src/provider_trace.rs` is in scope because role execution must
not persist provider-exchange traces even when a named profile enables ordinary
tracing. `tests/role_run_cli.rs` is the dedicated real-binary, disposable-home,
loopback-provider contract suite; keeping these cases separate avoids widening
the already broad ordinary CLI fixture.

Discovery note: `src/agent/stream.rs` is in scope because provider-controlled
text, reasoning, and tool-call accumulation needs role-specific aggregate
bounds before it can reach JSONL. `src/agent/tools.rs` is in scope because
role-run write safety must bind the checked inode and reject hardlinks/reopen
races at the actual mutation boundary. `src/mcp.rs` is in scope only for the
ignored actual-client compatibility test against `codex-team`; it does not
change ordinary MCP behavior.

Forbidden files/areas:

- `oracles/**`
- `reference/**`
- real runtime configuration, private credential directories, sessions,
  memory stores, installed binaries, and external services
- unrelated TUI snapshots or behavior

Runtime state affected: none during automated acceptance. Tests use disposable
homes, profiles, schemas, Git workspaces, fake providers, and child processes.

## Design and Invariants

### Public commands

```text
ca role-run --profile NAME --preflight
ca role-run --profile NAME
```

`NAME` must be a nonreserved named profile; `default` and `bare` are rejected.

Preflight emits one bounded JSON object:

```json
{
  "schema": "ca.role_run_preflight.v1",
  "ready": true,
  "profile": "codex-team",
  "sandbox_required": true,
  "routes": [{"provider":"openai","models":["gpt-5.6-sol"]}],
  "issues": []
}
```

It performs no provider request, does not resolve or print credential values,
does not migrate configuration, and does not create runtime state.
Every successfully emitted receipt exits zero, including an unhealthy receipt;
the typed `ready` and `issues` fields carry health.

Normal execution reads exactly one JSON object, capped at 1 MiB:

```json
{
  "schema": "ca.role_run.v1",
  "request_id": "worker-INT-148-001",
  "provider": "openai",
  "model": "gpt-5.6-sol",
  "reasoning_effort": "high",
  "workspace": "/canonical/clean/worktree",
  "prompt": "...",
  "output_schema": "/canonical/clean/worktree/.orchestrator/schemas/worker.schema.json",
  "tools": ["read_file", "list_dir", "search_files", "glob", "apply_patch", "write_file"],
  "grants": [{"kind":"tool","tool":"read_file"}]
}
```

Unknown fields and wrong schema identifiers fail before provider traffic.
Request IDs are safe ASCII and at most 128 bytes; prompt is at most 256 KiB;
tools are at most 32; grants are at most 64.

### Workspace and schema

- The workspace must already be canonical and equal the child current
  directory.
- The output schema must be a regular non-symlink file inside the workspace,
  with no symlink ancestor escape, and must pass existing structured-output
  validation.
- Provider/model must exist in the named profile. The reasoning value must be
  supported by that exact effective model route.
- Saved sandbox mode must parse strictly and equal `required`.

### Authority

- Force `HeadlessApprovalMode::RequireApproval`.
- Persistent Deny rules remain ceilings. Persistent Allow and Ask rules do not
  provide positive authority.
- Supplied process grants are the only positive authorization.
- Role capabilities disable persistence, configuration mutation, memory,
  learning, skills, plugins, MCP, LSP, hooks, subagents, workflows, jobs,
  browser, email, external communication, and all network exposure.
- On Unix, every existing tool-path component is enumerated for exact spelling
  and opened descriptor-relative with `O_NOFOLLOW`; execution repeats the
  descriptor walk and enforces type/link/device/depth/entry/byte bounds.
- A missing final Worker leaf is allowed. Private trees, protected roots for
  writes, symlinks, hardlinks, nonregular nodes, root replacement, and workspace
  escape fail closed.
- Worker mutation scans the complete resulting file for selected-credential
  material and publishes only through revalidated atomic replacement.
- Role-only patches are forward-only, cancellable, source/hunk/line/output
  bounded, and validate old/new coordinates including zero-count ranges.
- Local role tools fail closed where Unix directory-descriptor primitives are
  unavailable.
- Exact spelling has a narrow concurrent case-only rename race on
  case-insensitive filesystems; the authoritative descriptor walk still cannot
  follow an intermediate symlink or leave the workspace.
- There is no `allow_network` request field.
- Use `RequestKind::RoleRun`.

### Output and lifecycle

- Stdout contains only `ca.headless.v2` JSONL; diagnostics use bounded,
  sanitized stderr.
- Success requires exactly one object-valued `structured_output` event and a
  later `completion` event.
- The selected credential is an opaque redaction value across events,
  structured output, usage, diagnostics, continuations, and complete-result
  mutation scans.
- No session store or runtime-persistent extension manager is constructed.
- SIGINT/SIGTERM cancellation remains latched across await boundaries,
  terminates role-owned subprocess groups, reaps direct children, and returns
  the documented cancelled exit.

## Milestones

### M1 — Characterization and DTOs

- Add failing CLI/spec/preflight characterizations.
- Add deny-unknown-fields request/preflight DTOs and every byte/count/path
  bound in `src/role_run.rs`.
- Validate canonical workspace, contained regular schema, named profile, route,
  effort, and strict required sandbox.

Validation: `cargo test --bin ca role_run_ --no-fail-fast`.

### M2 — Offline configuration boundary

- Add a shape-only profile loader for preflight that never resolves secrets,
  reads legacy stores, migrates configuration, or creates directories.
- Add a no-write execution loader that resolves only the selected route at the
  execution boundary and keeps all diagnostics credential-free.

Validation: injected stores/environment canaries prove zero secret access,
zero provider traffic, and zero filesystem mutation during preflight.

### M3 — Restricted preparation and execution

- Add `RequestKind::RoleRun` and role-specific capabilities.
- Extract only the reusable bounded scheduled-worker validation/preparation
  pieces; do not expose the scheduled command.
- Skip plugin and optional-extension composition before construction.
- Filter persistent permission rules to Deny only and install exact process
  grants.
- Route through existing structured-output enforcement with an exact
  per-request reasoning override.

Validation: fake-provider advisory and Worker success/failure cases.

### M4 — Cancellation and public CLI

- Route leading `role-run` tokens without affecting the ordinary flat parser.
- Implement bounded stdin/stdout/stderr and signal-driven owned-group cleanup.
- Add `/.orchestrator/` to `.gitignore`.
- Publish `docs/ROLE_RUN_PROTOCOL.md`.

Validation: timeout/cancellation child-and-grandchild tests plus public CLI
help/preflight/execution tests.

### M5 — Cross-repository compatibility

- Build the real `ca` binary without installing it.
- Run orchestrator ignored/fake-provider compatibility against `CA_BIN` using
  disposable profiles and workspaces.
- Exercise every read-only role schema, one allowed Worker write, one rejected
  outside-scope write, timeout, and cancellation.
- Run actual `ca` `McpManager` discovery against the orchestrator MCP server.
- Do not run a live provider pilot without separate authorization.

## Acceptance Criteria

- Preflight is bounded, secret-free, provider-free, and mutation-free.
- Unknown fields, wrong schema, oversized input, reserved profile, invalid
  route/effort, non-required sandbox, workspace mismatch, schema symlink, and
  schema escape all fail before provider traffic.
- Persistent Allow/Ask cannot authorize a role operation; persistent Deny can
  still refuse it.
- Advisory roles cannot write. Worker writes require exact request grants,
  descriptor-rooted no-follow mutation, complete-result credential scanning,
  and remain subject to the orchestrator's independent post-run guard.
- Worker patches are bounded, cancellable, and validate both coordinate systems.
- Plugins, skills, MCP, LSP, hooks, agents, workflows, jobs, memory, browser,
  email, and network are absent from model-visible and executable authority.
- Successful stdout is valid ordered `ca.headless.v2` JSONL with one object
  result and completion; all other terminal states fail closed.
- Cancellation and timeout terminate owned process groups and reap direct
  children.
- Non-Unix local tool execution fails closed.
- Existing CLI/TUI/headless/schedule/service behavior and saved configuration
  compatibility remain unchanged.

## Validation

Focused:

```text
cargo test --bin ca role_run_ --no-fail-fast
cargo test --bin ca role_run_apply_patch_ --no-fail-fast
cargo test --test role_run_cli --no-fail-fast
cargo test headless::tests --no-fail-fast
cargo test permission::tests --no-fail-fast
cargo test schedule::tests --no-fail-fast
```

Completion:

```text
cargo test --no-fail-fast
cargo test --all-targets --all-features --no-fail-fast --quiet
cargo fmt --all --check
cargo clippy --all-targets --all-features --offline -- -D warnings
cargo build --release --all-features --offline
target/release/ca --help
target/release/ca --version
target/release/ca role-run --help
git diff --check
```

Also require no `.snap.new`, no `oracles/` or `reference/` change, and no
runtime-profile/session/credential/install mutation.

Manual/visual: none required for the machine protocol. The first real-provider
pilot requires separate user authorization.

## Progress

- [x] Create and validate clean snapshot baseline commit.
- [x] Record committed and dirty-checkout safety evidence.
- [x] Add role-run protocol, preflight, authority, output, and signal tests.
- [x] Implement offline shape-only profile preflight.
- [x] Implement strict role request validation and role-only capabilities.
- [x] Finish audited restricted execution and permission filtering, including
  exact-case descriptor-rooted read containment, bounded cancellable blame,
  malformed-Deny rejection, complete-result selected-credential scanning, and
  hardlink/race-safe role-only writes.
- [x] Add the role-only bounded, forward, cancellable patch engine with
  old/new-coordinate validation.
- [x] Finish public CLI, cancellation, and documentation, including routing,
  typed ready/unready preflight, provider/tool-flight owned-group cleanup, the
  public protocol page, and ignored runtime state.
- [x] Pass latest-source focused, formatting, strict Clippy, hygiene, release,
  and help/version gates.
- [ ] Pass the socket-backed final default/all-feature, real-binary CLI, and
  cross-repository matrices in an environment that permits loopback and the
  orchestrator isolation backend.
- [x] Record pre-final real-binary compatibility characterization without live
  traffic: every orchestrator role schema, allowed/denied Worker writes, and
  actual `ca::McpManager` observe/operate discovery/import/no-work polling.

## Decisions

- Decision: preserve the full current workspace in one validated checkpoint
  before role-run work.
  Evidence: 573 source/docs/test paths are deeply coupled; selective replay
  would not preserve the documented current behavior.
  Consequence: role-run changes begin from clean commit `abab2fb`.
- Decision: implement a dedicated public boundary instead of reusing the
  hidden scheduled command.
  Evidence: scheduled authority permits broader tools/network and has a
  different serialized contract.
  Consequence: share internal validators only where the narrower role contract
  remains explicit.

## Surprises and Discoveries

- The committed pre-snapshot HEAD already contained the core schedule,
  structured-headless, permission, and MCP foundations. The dirty changes were
  newer product work rather than required role-run prerequisites.
- Existing process-only configuration loading is not offline: it may resolve
  credentials and persist a legacy approval-mode migration.
- The generic tool path discovers and promotes a parent Git workspace and its
  read tools historically assume ordinary interactive trust. A role run must
  instead pin the request directory as the exact root and revalidate every
  read invocation for traversal, symlinks, and regular-file requirements.
- `blame` is a subprocess despite being a read tool. Role execution therefore
  needs the same bounded process-group, timeout, cancellation, and descendant
  reaping discipline as mutation-capable subprocesses.
- A nonzero exit for a well-formed unhealthy preflight conflicts with the
  orchestrator runner, which parses the typed receipt only after process
  success. Readiness now belongs exclusively in `ready` and `issues`.
- A valid provider response can echo a configured credential through otherwise
  valid structured output or usage metadata. Role execution therefore carries
  the selected credential only as an opaque redaction value across every
  emitted/continued surface.
- Exact path grants do not prevent mutation through a pre-existing hardlink or
  a check/open race. Role writes therefore require a no-follow, single-link,
  inode-checked, same-directory atomic replacement boundary and fail closed
  where the required host primitives are unavailable.
- Case-insensitive filesystems require explicit directory enumeration because
  an ordinary successful path open does not prove exact spelling. Enumeration
  followed by descriptor open leaves a narrow concurrent case-only rename race;
  descriptor containment still prevents symlink traversal and workspace escape.
- The ordinary interactive patch helper was not suitable for untrusted role
  patches. Role execution now has a separate forward-only parser with explicit
  source/hunk/line/output bounds, cancellation polling, and both-coordinate
  validation.
- Scanning only each write argument misses a credential assembled over multiple
  edits. The mutation boundary now scans the complete resulting file before
  publishing it.
- The managed acceptance host denies loopback listener creation with EPERM.
  Socket-backed CLI/default/all-feature/cross-repository gates must be rerun in
  a host that permits their local transports; failures are not reclassified as
  passes.

## Recovery and Rollback

- The original `/Users/aji/project/ca` checkout remains the untouched source
  workspace.
- The clean checkpoint is a normal Git commit; role-run changes remain after it
  as reviewable worktree edits until separately authorized for commit.
- No reset, clean, force checkout, installed-binary replacement, or runtime
  state mutation is part of recovery.
- If a test child survives unexpectedly, identify only the test-owned process
  group and stop it; never use broad process-name killing.

## Completion Record

Changed files: implementation remains uncommitted and includes `.gitignore`,
`src/{config,headless,main,mcp,permission,provider_trace,request,role_run,
runtime}.rs`, `src/agent/{stream,tools}.rs`, `tests/role_run_cli.rs`, this
plan, the public protocol, and the harness pages. The clean commit review and
environment-blocked aggregate/cross-repository gates remain pending.

Latest-source commands and results:

- `cargo test --bin ca role_run_ --no-fail-fast`: PASS, 34/34.
- Patch-coordinate focused tests: PASS, 2/2.
- `cargo clippy --all-targets --all-features --offline -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- tracked/untracked whitespace, `.snap.new`, and `oracles/`/`reference/`
  immutability checks: PASS.
- `cargo build --release --all-features --offline`: PASS.
- release `--help`, `--version`, and `role-run --help`: PASS.
- final release candidate SHA-256:
  `f159fa419d8ac3061a6d905617996c58a27a268ca7254df8b3d666d39d184222`.
- `cargo test --test role_run_cli --no-fail-fast`: ENVIRONMENT BLOCKED;
  3 provider-free tests passed and 14 socket-backed tests failed at
  `TcpListener::bind` with `Operation not permitted` before provider traffic.

The earlier pre-final default matrix (1,692 main, 37 ordinary CLI, and 10 role
CLI tests) and pre-final real-orchestrator/MCP compatibility runs remain useful
characterization, but they predate the final descriptor, credential, and patch
hardening and are not final acceptance evidence. The current managed sandbox
also blocks the local transports needed to rerun them.

Remaining risks: the narrow case-only rename/spelling-check race on
case-insensitive filesystems, final aggregate and cross-repository regressions
not runnable in this sandbox, clean commit review, and the separately
supervised dogfood sequence. Descriptor-rooted no-follow traversal still
prevents that spelling race from becoming a symlink or workspace escape.
Live-provider behavior remains deliberately unproven and is not an automated
acceptance requirement.

Superseding completion evidence (2026-08-22): after monorepo consolidation,
all 42 focused role-run unit tests and all 17 socket-backed role-run CLI tests
passed. The full `ca` and workspace suites, strict workspace Clippy,
formatting, release build, help/version, diff, and snapshot gates also passed.
No live provider or real-profile mutation ran. Clean-commit review remains an
ordinary repository-owner decision, not an incomplete implementation gate.
