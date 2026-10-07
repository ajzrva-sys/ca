# LSP-065 — Semantic Navigation

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Expose bounded, read-only semantic navigation tools backed only by language-server executables the user explicitly configured. Start servers lazily, keep them process-scoped, translate user-facing 1-based positions to LSP UTF-16 positions, reject locations outside workspace authority, and clean every child process group on timeout or shutdown.

## Non-Goals

- Installing, downloading, or auto-detecting language servers.
- Editing source through LSP, executing server commands, applying workspace edits, code completion, diagnostics UI, or an `/lsp` manager.
- Network access, external document URIs, or sharing LSP state with sessions/subagents.
- Cursor parity. This is an oracle-informed `ca` extension.

## Evidence and Current Behavior

- `src/config.rs` has provider, MCP, hook, and skill configuration but no LSP configuration.
- `src/agent/registry.rs` has no semantic-navigation tools.
- `src/main.rs::execute_tool_dispatch_unhooked` dispatches built-ins, work tools, agents, and MCP; no LSP path exists.
- `src/mcp.rs::spawn_stdio_service` is the existing safe pattern for resolving a configured executable, applying `SandboxBackend`, clearing the environment, creating a process group, bounding stderr, and cleaning it.
- `src/workspace.rs::WorkspaceContext::resolve` is the authority boundary for model-supplied paths.
- `src/runtime.rs::RunCapabilities` disables MCP, skills, hooks, persistence, and config writes for bare mode; LSP must follow the extension-disabled boundary.
- Inferred: a small JSON-RPC client over stdio is preferable to importing a full editor-oriented LSP stack because only six request families and explicit lifecycle ownership are needed.

## Scope

Allowed files: `src/{config,lsp,runtime,main,app,headless,tool_selection}.rs`, `src/agent/registry.rs`, focused tests/snapshots, this plan, and required harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, user config, Keychain, saved sessions, installed binaries, automatic executable discovery/download, and unrelated source.

Runtime state affected: only child language-server processes and process-memory document/server caches. Hermetic validation uses temporary workspaces, homes, and fake executables.

## Design and Invariants

- Add default-empty `[lsp.servers.<id>]` records with `command`, `args`, `language_id`, `extensions`, `enabled`, and bounded `timeout_secs`. Config debug output exposes counts/booleans, never environment values (none are accepted).
- Resolve an absolute existing command or one exact basename through the inherited safe `PATH`. Reject relative paths containing `/`. Never probe for conventional executables.
- `LspManager` owns one lazily initialized stdio process per server. Calls for one server serialize; distinct servers may run independently.
- Spawn under the effective sandbox with workspace roots, a dedicated temporary directory, network denied, a cleared allowlisted environment, and an independent process group. Bound and sanitize stderr; never put it in success output.
- Initialize with the primary root and workspace folders. Keep opened-document text/version process-only and send `didOpen` or `didChange` before a query.
- User lines/columns are 1-based Unicode-scalar positions. Convert the prefix to a zero-based UTF-16 code-unit count. Reject invalid line/column positions before server traffic.
- Accept only `file:` URIs canonically contained by current workspace roots. Any external/non-file location makes the request fail safely; it is never partially displayed.
- Bound document input to 1 MiB, protocol headers to 8 KiB, frames to 1 MiB, result collections to 100 items, and model-visible output to 64 KiB with explicit omission counts.
- Six Main-only read-only tools: document symbols, definition, references, hover, implementation, and incoming/outgoing call hierarchy. All require an exact configured server ID; position tools additionally require a workspace path and 1-based line/column.
- Central tool selection removes all `lsp_*` tools when LSP is unavailable. Bare, review, plan, checkpoint, scripted, and subagent contexts do not receive them.
- No request URI, absolute workspace path, server stderr, command path, or environment value appears in model-visible failures. Results use workspace-relative paths.
- Request timeout, malformed framing, early exit, explicit shutdown, and manager drop cleanup terminate the exact owned process group and remove its temporary directory.

## Milestones

### M1 — Characterize config, tools, and coordinate boundary

- Add failing tests for default-empty config, safe configuration validation, static tool visibility constraints, Unicode-to-UTF-16 conversion, and external URI rejection.
- Validation: focused `config`, `tool_selection`, and `lsp` tests fail for missing behavior only.

### M2 — Implement bounded LSP lifecycle and requests

- Add configuration and the process-scoped JSON-RPC stdio manager.
- Implement initialize/open/change/request/shutdown framing, bounds, safe errors, workspace URI conversion, and output normalization.
- Add fake-server tests covering all six request families, malformed/timeout/oversized data, invalid coordinates, external URIs, executable resolution, network-denied sandbox construction, and process cleanup.

### M3 — Integrate all ordinary runners and tool policy

- Add registry schemas, central selection filtering, TUI/headless manager construction, dispatch, and shutdown.
- Verify ordinary requests expose tools only with configured enabled servers; bare and internal profiles do not.
- Add deterministic wide/narrow generic tool-card snapshots for success and safe failure.

### M4 — Gates, walkthrough, and documentation

- Run focused suites, all repository tests, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Exercise a released headless run against a fake server under a temporary home/workspace; verify the process exits and temporary state is removable.
- Reconcile architecture, continuity, testing, next steps, parity, environment/index documentation; archive this plan.

## Acceptance Criteria

- No configured LSP servers means no `lsp_*` schemas and no processes.
- One explicit tool call starts only the named configured executable and sends one correctly converted request after initialization/document synchronization.
- Astral Unicode before a requested column counts as two UTF-16 code units.
- Paths and returned locations outside workspace roots fail without revealing the external URI.
- Symbol/location/hover/call results are deterministic, workspace-relative, bounded, and terminal-safe.
- Timeout, cancellation/shutdown, malformed frames, and server exit leave no owned child process group.
- LSP tools are absent in bare, review, checkpoint, plan, scripted, and subagent contexts.
- Existing configs load unchanged; missing LSP configuration is inert.

## Validation

Focused: pending.

Completion: pending.

Manual/visual: pending.

Blocked checks: none.

## Progress

- [x] Source, lifecycle, tool-selection, workspace, and sandbox seams characterized.
- [x] Failing focused characterization tests added.
- [x] Configuration and manager implemented.
- [x] Registry, runner, dispatch, and shutdown integration implemented.
- [x] Focused and repository-wide gates passed.
- [x] Temporary-home fake-server walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: make server selection explicit in every tool call instead of guessing by file extension.
  Evidence: multiple configured servers may claim one extension, and the roadmap permits only configured executables.
  Consequence: behavior is deterministic and no hidden process starts occur.
- Decision: treat any external/non-file returned URI as a failure, not an omitted partial result.
  Evidence: the roadmap says to reject external URIs.
  Consequence: the model cannot infer hidden external paths from partial output.
- Decision: expose LSP only to the Main tool profile.
  Evidence: review/checkpoint/subagent runners have intentionally restricted tool sets and no shared lifecycle owner.
  Consequence: no implicit semantic server expansion in isolated internal contexts.

## Surprises and Discoveries

- None yet.

## Recovery and Rollback

Remove the default-empty LSP config field, `src/lsp.rs`, six registry entries, manager fields/dispatch wiring, and LSP selection constraint. No persisted migration or runtime state rollback is required. Stop fake processes by exact recorded process-group ID and remove only their dedicated temporary directories.

## Completion Record

Changed files: `src/{lsp,config,runtime,main,app,headless,tool_selection}.rs`, `src/agent/{registry,tools,subagent}.rs`, `src/tui/render_tests.rs`, four snapshots, harness documents, and this plan.

Commands and results: focused LSP/registry/selection/render suites passed; `cargo test --no-fail-fast` passed 748 unit plus 28 CLI tests; formatting, snapshot hygiene, strict Clippy, release build, help/version/bare-profile smoke passed; isolated released-headless fake-provider/fake-LSP walkthrough passed with exact UTF-16 request and cleanup.

Remaining risks: real language servers vary in initialization capabilities and result extensions; the bounded client intentionally rejects unsupported or external-location shapes instead of attempting editor-grade compatibility. No automatic server discovery or installation exists.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` reconciled.
