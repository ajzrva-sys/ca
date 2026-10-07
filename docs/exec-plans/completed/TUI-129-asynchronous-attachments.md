# TUI-129 — Asynchronous `@` Attachments

Status: COMPLETE (2026-08-16)
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Replace synchronous composer file discovery with cancellable bounded workspace
indexing and fuzzy/frecency ranking, add send-time validated line-range file
references, and present already-discovered MCP resources without activating a
server merely by opening the picker.

## Non-Goals

- Do not add a drawer, background MCP connection, home-directory scan, or
  persist resource bodies.
- Do not copy MiMo source or modify `oracles/`/`reference/`.
- Do not expand workspace, symlink, MCP approval, or network authority.
- Do not implement later roadmap tasks in this plan.

## Evidence and Current Behavior

- `src/main.rs` currently opens the picker by synchronously walking a single
  current directory and retaining a small path list in `AppState`.
- `src/tui/input.rs` filters and inserts plain `@path` text, while submission
  expands file mentions through the existing workspace authority boundary.
- `src/workspace.rs` owns declared roots, read-only labels, containment, and
  symlink-safe preparation.
- `src/mcp.rs` already retains discovered text-resource metadata and owns all
  later activation, origin approval, transport, and network gates.
- `ignore` and `nucleo` are existing dependencies. MiMo is read-only behavioral
  evidence, not a source or Cursor parity requirement.

## Scope

Allowed files: a focused attachment/index module; composer/app/event state;
TUI input/render; request-time attachment resolution; workspace/MCP metadata
adapters where required; session-compatible inert references; tests/snapshots;
and repository harness documents.

Forbidden: oracle/reference edits, user runtime mutation during validation,
unprompted MCP activation, resource-body persistence, unrelated refactors,
external agents, and GUI surfaces.

## Design and Invariants

- A blocking worker traverses every declared root with `ignore`, stops at
  50,000 paths, and sends generation-tagged snapshots; opening/filtering never
  blocks the TUI, and stale generations cannot replace current state.
- Presentation retains at most 250 matches. Ranking uses fuzzy score first and
  profile-scoped workspace-relative frecency second. Persistence retains at
  most 1,000 relative identities and never unrelated absolute paths; bare mode
  keeps only process memory.
- Typed file references support no range, one line, or inclusive ordered line
  ranges. Submission revalidates root containment/read-only identity, regular
  file/no-follow policy, current size, and current content before inclusion.
- Already-discovered MCP resource metadata appears separately without connect,
  list, read, or activation calls. Selection inserts a typed process-only
  reference; send-time resolution reuses existing MCP origin approval/network
  gates. Sessions retain only textual references or unavailable markers.
- Every list, label, query, path fragment, range, file body, and resource result
  has an explicit byte/item bound and terminal sanitization.

## Milestones

### M1 — Typed references and asynchronous index

- Characterize current discovery, composer parsing, submission expansion, and
  multi-root authority.
- Add parsing/validation/range tests and a generation-tagged bounded index.
- Replace synchronous traversal without changing ordinary composer behavior.

### M2 — Ranking and durable frecency

- Add deterministic nucleo-based fuzzy ranking and relative frecency tie-breaks.
- Add bounded profile store, bare behavior, corruption recovery, and atomic
  owner-only/symlink-safe writes.

### M3 — MCP metadata and send-time resolution

- Expose only already-known resource metadata in a distinct picker section.
- Preserve non-activation at open/filter and route selected references through
  existing approval/network gates at submission.
- Keep durable data inert and body-free.

### M4 — Interaction, rendering, and completion

- Add deterministic wide/narrow loading, multiple-root, no-match, range, MCP,
  stale/error, and capped snapshots plus input tests.
- Run focused and repository completion gates and a disposable native PTY.
- Reconcile harness documents and archive this plan before TUI-130 starts.

## Acceptance Criteria

- Opening `@` remains responsive on large/ignored/multi-root workspaces and a
  newer query/index generation always wins.
- File matches are bounded, fuzzy-ranked, frecency-aware, and labeled by root;
  `@path#12` and `@path#12-40` include only currently validated content.
- Drift, malformed/reversed/out-of-file ranges, oversized/special files,
  traversal, and disallowed symlinks fail before provider dispatch.
- MCP rows require no server work to display and bodies are fetched only during
  explicit send-time resolution under existing gates.
- Old input/session/config behavior remains readable and no oracle, credential,
  installed binary, or real user state changes.

## Validation

Focused:

- `cargo test attachments::`: PASS, five parsing, validation, discovery,
  ranking, frecency, corruption-recovery, and storage-bound tests.
- `cargo test attachment_picker_`: PASS, typed input plus eight reviewed
  wide/narrow loading/results/no-match/error snapshots.
- Stale-generation orchestration and fake-MCP lazy metadata/send-time resource
  resolution tests: PASS.

Completion:

- `cargo test --no-fail-fast`: PASS, 1,488 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS, 1,517 main,
  118 experiment, and 37 CLI tests; two ignored.
- `cargo fmt --all --check`, strict all-target/all-feature Clippy,
  `cargo build --release --all-features`, help/version, `git diff --check`, and
  snapshot hygiene: PASS.

Manual/visual: PASS. A released bare/no-session 80x24 PTY showed the transient
indexing phase, two ignore-filtered workspace results, fuzzy narrowing to
`src/main.rs`, and insertion of `@src/main.rs#2-3`. The disposable home stayed
empty. The stopped fixture moved recoverably to
`/Users/aji/.Trash/ca-tui129-pty.hqK18T`.

Blocked checks: none currently.

## Progress

- [x] Current picker/request/workspace/MCP boundaries characterized.
- [x] Typed asynchronous index and cancellation complete.
- [x] Fuzzy/frecency and line-range validation complete.
- [x] MCP metadata-only picker and send-time resolution complete.
- [x] TUI snapshots/interactions and completion gates complete.

## Decisions

- Decision: keep attachment authority at submission, not picker selection.
  Evidence: files and MCP availability can change while a draft remains open.
  Consequence: selection is inert and every send receives current validation.
- Decision: persist only root-relative frecency identity.
  Evidence: ranking needs stable reuse without building a cross-workspace path
  history.
  Consequence: removed roots naturally hide records and bare mode stays
  process-only.

## Surprises and Discoveries

- An empty valid workspace index cannot use vector emptiness as its "not yet
  indexed" signal; an explicit readiness bit prevents an infinite rediscovery
  loop.
- The ignore walker requires `require_git(false)` to honor a standalone
  `.gitignore` outside a Git repository, which matters for additional roots and
  disposable workspaces.
- Manual path resolution canonicalizes symlinks, so no-follow validation must
  inspect lexical components before canonical workspace resolution.

## Recovery and Rollback

Index workers and MCP references are generation-scoped process state. Frecency
storage is versioned and optional; a targeted rollback can ignore it without a
session migration. Never use destructive Git reset/checkout for recovery.

## Completion Record

Changed files: `src/attachments.rs`, `src/{app,main,mcp,runtime}.rs`,
`src/tui/{commands,input,input_tests,render,render_tests}.rs`, new deterministic
snapshots, and repository harness documents.

Commands and results: recorded above and in `TESTING.md`.

Remaining risks: MCP resource bodies are intentionally process-only and depend
on the existing server-origin/network boundary at send time. Picker discovery
does not start a server, and races yield an unavailable marker.

Harness updates: architecture, continuity, testing evidence, knowledge index,
and executable queue updated before activating TUI-130.
