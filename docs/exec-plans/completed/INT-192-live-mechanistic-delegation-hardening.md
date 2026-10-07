# INT-192 — Live Mechanistic Delegation Hardening

Status: COMPLETE
Owner/session: current Codex session
Last updated: 2026-08-27

## Goal

Make the installed CA-to-codex-team delegation path accept useful bounded
searches, preserve actionable remote errors, and retry failed provider
continuations without replaying already resolved tools.

## Non-Goals

- No natural-language host classifier or forced tool choice.
- No delegation in role-run, subagent, review, Plan, checkpoint, scheduled,
  service, or bare surfaces.
- No product edits through the mechanistic facade, catalog-count changes,
  profile mutation, live provider traffic, commits, or protected-tree edits.

## Evidence and Current Behavior

- A real installed TUI run reached codex-team and completed
  `list_tracked_files`, proving the stdio integration works.
- `prepare_tool_invocation` maps every registry validation failure to
  `WorkspaceError::Missing`, producing the false “workspace path does not
  exist” diagnosis.
- `SearchText.paths` structurally accepts relative paths but execution requires
  every entry to be one exact tracked regular file; matching is literal only.
- `call_host_tool_structured` discards structured content whenever MCP sets
  `is_error`, so the facade reports a generic protocol failure.
- `/retry` branches at the initiating user message, discarding completed tool
  calls and outputs from the new branch.

## Scope

Allowed files: the shared protocol crate; codex-team mechanistic query/MCP
implementation and package manifest; CA workspace/MCP/facade/prompt/retry
state and adjacent tests; current INT-192 control documentation.

Forbidden: `oracles/`, `reference/`, user profiles/configuration, credentials,
services, unrelated dirty files, Git cleanup/reset/stash, and commits.

Runtime state affected: validated release binaries may be replaced
recoverably only after all completion gates pass. The existing named profile
must remain byte-identical.

## Design and Invariants

- Search remains literal by default and adds explicit Rust-regex matching.
- Search scopes are matched only against Git’s tracked-file list. Relative
  exact files and component-bounded directory prefixes are accepted; no
  filesystem walk or absolute-path normalization is introduced.
- Typed MCP tool failures remain distinct from transport and protocol errors,
  are bounded/redacted before exposure, and never grant fallback except for
  the existing unavailable/declined prefer states.
- Retry preserves the longest suffix ending in a fully resolved tool batch
  from the failed active turn, excludes failed partial assistant prose, and
  starts a tool continuation. With no resolved batch, existing whole-turn
  replay remains.
- CA’s launch workspace remains independent of the repository pinned by the
  mechanistic server.

## Milestones

### M1 — Search contract and guidance

- Add literal/regex mode and tracked file/prefix search scopes.
- Describe repository-relative fields and add enabled-Main prompt guidance.
- Cover compatibility and repository-bound search behavior.

### M2 — Truthful errors

- Separate invalid tool arguments from missing paths.
- Preserve typed MCP errors and classify facade results truthfully.
- Cover typed-error, transport, protocol, and UI-status behavior.

### M3 — Receipt-preserving retry

- Introduce a process-local retry boundary for resolved tool continuations.
- Preserve source immutability and `--no-session` behavior.
- Prove resolved tools are not called twice.

### M4 — Completion and delivery

- Run focused tests during iteration, then the completion matrix once.
- Update durable documentation, install both validated binaries recoverably,
  and verify the unchanged named profile without provider traffic.

## Acceptance Criteria

- A Main agent launched outside the pinned repository can issue one relative
  delegated query without discovering an absolute repository path.
- Exact-file, directory-prefix, literal, and regex searches return bounded
  tracked-file results while unsafe/untracked paths remain denied.
- Invalid arguments, typed tool failures, protocol failures, and transport
  failures are distinguishable and rendered truthfully.
- `/retry` after any fully resolved tool batch sends only a continuation and
  performs zero duplicate tool executions; pre-tool failures still replay.
- Every excluded recursion surface remains facade-free and catalogs remain
  exactly 11/15/28 with zero resources.

## Validation

Focused: protocol, mechanistic query/MCP, workspace, facade/tool selection,
and retry/session tests.

Completion:

    cargo test --workspace --no-fail-fast -- --test-threads=1
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo build --workspace --release --all-features
    git diff --check

Also run snapshot/protected-tree hygiene, release help/version, production
isolation, exact MCP compatibility, installed hash checks, `/delegate`,
`/jobs`, and clean stdio shutdown without a live provider request.

## Progress

- [x] Characterize the live failures and trace their owning boundaries.
- [x] Complete M1 search contract and guidance.
- [x] Complete M2 truthful errors.
- [x] Complete M3 receipt-preserving retry.
- [x] Complete M4 validation, installation, and documentation.

## Decisions

- Decision: keep literal search as the default and add explicit regex mode.
  Evidence: existing callers serialize only `pattern`, while the live model
  naturally emitted an alternation-looking pattern.
  Consequence: old requests retain exact behavior and new requests can state
  their matching intent.
- Decision: keep CA’s launch workspace separate from the pinned repository.
  Evidence: the facade already gets revision and repository identity from the
  configured host server.
  Consequence: guidance must prevent absolute-root discovery and all action
  paths remain repository-relative.
- Decision: preserve every fully resolved tool batch on retry, not only
  delegation receipts.
  Evidence: replaying an already successful mutating tool is more dangerous
  than replaying a read.
  Consequence: retry becomes a tool continuation whenever a safe resolved
  boundary exists.

## Surprises and Discoveries

- The generic workspace preparation layer, rather than delegation-specific
  code, owns the misleading missing-path message.
- The remote server already sends bounded structured errors; CA alone drops
  them at the host-call boundary.
- The real 11/15/28 compatibility fixture can bind codex-team to one repository
  while CA's own workspace is a different temporary directory; relative
  directory-prefix regex search succeeds without repository discovery.
- A retry-safe boundary is representable by the last fully resolved assistant
  tool batch because successful, failed, and denied tools all retain terminal
  status plus output in that canonical message.

## Recovery and Rollback

Revert only INT-192 hunks after comparing the recorded pre-edit SHA-256 values.
Do not reset or clean the shared worktree. Installed binaries, if reached, are
replaced atomically after timestamped hashed backups and can be restored from
those exact backups.

## Completion Record

Changed files: shared mechanistic protocol/schema; codex-team query, executor,
MCP classification, dependency manifest, and tests; CA registry, selection,
workspace preparation, MCP transport, facade, prompt construction, retry state,
headless/TUI adjacent tests; Cargo lockfile; INT-192 harness documents. No
protected-tree, user-profile, credential, or session file changed.

Commands and results: focused protocol/query/error/facade/selection/retry tests
PASS. The serial workspace run passed every target except one stale assertion
that expected generic `REPO_QUERY_FAILED` for revision drift; after changing
only that expectation to the intentional `STALE_REVISION`, the entire failed
codex-team library target passed 586 tests with one ignore. The already-passing
targets were 2,080 CA tests with two ignores, 38 CLI, 17 role-run, 15+3
ca-watch, 13 codex-team integration, and four protocol tests. Formatting and
strict all-target/all-feature Clippy each exposed one local mechanical issue,
were corrected, and pass. The all-feature release build, help/version,
diff/snapshot/protected hygiene, production isolation, exact release 11/15/28
MCP fixture, and reverse release-CA role-run fixture pass.

Installed release hashes are
`ca=c37240f55d320e02e1bf66d8968134ac2b61914d910f3ab0ce20d7f7bf588061`
and
`codex-team=aba99855744d39d838e77a897457df7e1560f69912c8c8b23d4035b129efc479`.
Recoverable predecessors are
`/Users/aji/.local/bin/ca.backup-int192-20260827-001414-b966d7fd` and
`/Users/aji/.local/bin/codex-team.backup-int192-20260827-001414-770a000f`.
Signatures, help/version, `/delegate` (15 tools, zero resources), `/jobs`, and
clean child shutdown pass. The named profile remains mode 0600 and byte-identical
at SHA-256
`2bc81df31bf284208e417fa34240016c6d0e240a5759e9b4f80f90cdb8b74a96`.

Remaining risk/procedural incident: a post-install smoke used positional
`doctor` instead of the local `--doctor` flag. CA interpreted it as a headless
prompt and made one unintended provider turn, which completed two read-only
mechanistic calls before interruption with exit 130. No session file, profile,
repository target, or provider configuration changed. The correctly spelled
`--doctor` then passed offline and started no MCP child. Consequently the
implementation's no-provider behavior is covered, but the overall manual
validation session did not satisfy the intended zero-live-provider condition.

Harness updates: `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and this
completed ExecPlan record behavior, validation, installation, recovery, and
the procedural incident.
