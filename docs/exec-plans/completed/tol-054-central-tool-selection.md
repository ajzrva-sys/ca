# TOL-054 — Central Tool Selection

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Resolve every runner's model-visible tools through one typed `ToolSelectionPolicy`, expose repeatable `--tools`, `--include-tools`, and `--exclude-tools` CLI controls, and reject unknown names before provider traffic without widening any existing profile, approval, workspace, sandbox, or network boundary.

## Non-Goals

- Rich command-pattern matchers, persisted tool presets, a `/tools` manager, runtime filter switching, permission-rule changes, or tool-specific argument restrictions; APR-055 owns richer policy.
- Deferred MCP startup/search, remote MCP transport, provider protocol changes, or new tools.
- Renaming current snake_case tool names, adding deprecated oracle aliases, or claiming Ante/Cursor runtime parity.
- Making tool exposure imply execution approval, network authority, workspace access, or sandbox bypass.

## Evidence and Current Behavior

- `src/agent/registry.rs::ToolRegistry::definitions` filters only by `ToolProfile`; there is no shared session selection type.
- Interactive preview uses `src/main.rs::filter_tools_for_mode`, while the actual async request independently rebuilds registry definitions after MCP catalog loading and reapplies only custom-mode filtering. Inferred and reproduced by the REQ-053 isolated walkthrough: bare mode advertised 35 tools, including `activate_skill`, even though skills were disabled.
- `src/headless.rs::run_with_backend` replaces the caller's filtered definitions with the full MCP catalog on each turn, creating the same capability bypass.
- Review, checkpoint, provider subagents, scripted execution, and subagent grant validation each construct registry definitions independently in `src/{review,main,test}.rs` and `src/agent/subagent.rs`.
- Ante documents replacement, then inclusion, then exclusion; case-insensitive tool-name matching; and identical filtering for dynamically registered MCP tools at `oracles/ante-main/docs-site/docs/reference/{cli-reference,architecture,tools-reference}.mdx`. These are documented product lessons, not observed runtime behavior or Cursor parity evidence.
- Inferred: profile eligibility must remain an immutable upper bound. A run policy may remove tools from Review/Subagent or select eligible tools, but cannot make a Main-only mutating tool eligible for Review or a tool eligible for Checkpoint.

## Scope

Allowed files:

- new `src/tool_selection.rs`
- `src/{main,app,headless,review,test}.rs`, `src/agent/{registry,tools,subagent}.rs`, and focused tests
- CLI integration tests and request/context characterization tests
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `docs/.INDEX.md`, and this ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, provider/config/session schemas, real user runtime state, Keychain, installed binary, external services, and unrelated future roadmap slices
- approval-rule behavior, execution authorization, MCP configuration, provider request serialization, and tool schemas/handlers

Runtime state affected:

- One immutable process-only `ToolSelectionPolicy` derived from CLI arguments. It is neither persisted nor placed in sessions.
- Validation uses fake backends, fake MCP registries, and disposable homes/loopback servers only.

## Design and Invariants

- `ToolSelectionPolicy` owns an optional replacement list plus inclusion and exclusion lists. CLI values are repeatable and comma-delimited per occurrence so positional prompts and `@file` parsing remain unambiguous.
- Names match canonical registered tool names case-insensitively and are normalized to their canonical spelling. Empty values, malformed names, and duplicates are handled deterministically; duplicates collapse without changing registry order.
- Resolution starts from tools eligible for the runner's `ToolProfile`. If replacement is present, start with matching eligible names; otherwise start with every eligible default. Add matching eligible inclusions, then remove exclusions. Final definitions always preserve registry order.
- Globally known but profile-ineligible names do not widen that profile and are omitted. A name unknown to the assembled registry is an error. This lets one process policy safely span Main, Plan, Review, Checkpoint, and Subagent without making a Main selection break an intentionally tool-free checkpoint.
- `ToolSelectionConstraints` are restrictive and applied after policy: model tool support, skill activation capability, custom-mode allowlists, review's exact read/reporter allowlist, and subagent grants. No constraint can add a tool.
- Explicit nonempty replacement/include selection against a model that declares `supports_tools = false` fails before provider traffic; an unfiltered toolless model continues with an empty tool list.
- Static built-ins are prevalidated during CLI runtime setup. Names not found there remain possible MCP names and are resolved only after the existing local MCP catalog is assembled. Final failure still occurs before `PreparedModelRequest` reaches the backend. MCP failure fallback cannot silently turn an unresolved requested MCP name into an unfiltered built-in request.
- Every production runner calls the same resolver. Interactive/headless/scripted main requests receive CLI policy. Reviews, checkpoints, and provider subagents receive the same run policy intersected with their immutable profile/allowlist/grants. Provider probes and `/btw` remain intentionally tool-free and do not invoke selection.
- Subagent grant creation validates against the resolver's effective Subagent set, so a parent cannot grant a tool hidden by the run policy. Existing exact grant and nested-agent restrictions remain authoritative.
- `activate_skill` is removed whenever `RunCapabilities.skills` is false, including after MCP catalog assembly. Context preview uses the same built-in resolution and actual prepared estimates classify origins from registry metadata rather than comparing against an already-filtered name set.
- Selection changes only schemas advertised to the model. Tool execution still validates the registry entry and traverses existing approval, workspace, sandbox, job ownership, and network boundaries.
- Tool-selection failures are typed, bounded, terminal-safe, and contain only unknown user-supplied names plus available canonical names. They never contain arguments, endpoints, credentials, MCP output, or provider bodies.

## Milestones

### M1 — Characterize current bypasses and CLI grammar

- Add failing tests for bare actual requests, headless MCP replacement, run-wide subagent/review constraints, positional prompts around each new flag, repeats/comma values, duplicates, case normalization, exclusion precedence, and unknown-name zero-provider-call behavior.
- Record exact current tool counts/order and no-tools model behavior without changing schemas.
- Validation: focused tests fail only on the known decentralized selection paths.

### M2 — Implement the resolver and CLI policy

- Add `ToolSelectionPolicy`, restrictive constraints, typed errors, and registry-order resolution.
- Parse the three flags without changing positional prompt or attachment behavior; reject malformed values before runtime construction and preserve potential MCP names for final catalog resolution.
- Add unit/property-style cases for replacement → include → exclude, profile upper bounds, casing, duplicates, empty selections, skills/custom/grants, and model support.
- Validation: resolver and CLI tests pass with no provider/network I/O.

### M3 — Migrate every runner and fix context attribution

- Route interactive preview/actual, headless, review, checkpoint, scripted, provider-subagent, and subagent grant validation through the resolver.
- Resolve against the final built-in plus MCP registry exactly once per request and return typed setup failure before backend invocation.
- Use registry origins for prepared built-in/MCP context estimates; prove bare advertises no `activate_skill` and zero MCP tools without an MCP catalog.
- Validation: caller characterizations cover all named runner profiles and no production `.definitions(...)` call remains outside the resolver/registry tests.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, full tests, formatting, strict Clippy, release build, help/version/profile and flag-order smoke tests, plus zero-pending-snapshot checks.
- With a disposable home and credentialless loopback provider/MCP fixture, inspect `/request` after default, replacement, include/exclude, bare, and unknown-tool runs. Confirm exact tool names/order, no provider POST for unknown names, no policy persistence, and unchanged approval behavior.
- Reconcile the five harness documents, archive this plan, and activate APR-055 only after every gate passes.

## Acceptance Criteria

- Replacement, inclusion, and exclusion occur in that exact order; matching is case-insensitive, duplicates collapse, exclusion wins, and output stays in canonical registry order.
- Profile eligibility and restrictive capability/grant/mode allowlists cannot be widened by CLI policy.
- Built-in and MCP tools pass through the same final resolver. A restrictive replacement cannot be bypassed by dynamic catalog assembly or catalog fallback.
- Unknown names fail before provider traffic in interactive, headless, review, and subagent paths with safe deterministic output.
- Bare actual and preview requests omit `activate_skill`; tool-free models advertise no tools and reject explicit attempted selection before traffic.
- Review remains read-only with its reporter only when selected; checkpoint remains tool-free; subagents receive only the intersection of profile, run policy, capability restrictions, and exact grants.
- Tool visibility does not alter approval decisions, workspace roots, sandboxing, network grants, handler validation, or provider Chat Completions serialization beyond the intended `tools` array.
- Existing positional prompts and `@file` arguments work before and after every new flag.
- Focused/full gates and isolated walkthrough pass without touching real runtime state, oracle files, external services, or the installed binary.

## Validation

Focused:

- PASS: resolver, CLI parser, interactive, headless, scripted, review, checkpoint, subagent, MCP, model-capability, bare, and unavailable-provider-call tests.
- PASS: reviewed wide/narrow safe tool-selection failure snapshots; zero pending snapshot files.

Completion:

- `cargo test --no-fail-fast`: PASS, 635 unit tests plus 26 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/profile and positional/attachment flag-order smoke: PASS.

Manual/visual:

- PASS with a disposable home/workspace and credentialless loopback provider. The filtered TUI `/request` surface showed exactly `read_file` then `search_files`; bare omitted `activate_skill`; an unknown name made no provider request; and a selected `write_file` still required approval and created no file.

Blocked checks:

- None currently.

## Progress

- [x] Traced existing registry/profile construction across interactive, headless, review, checkpoint, scripted, subagent, skills, and MCP paths.
- [x] Reviewed Ante's documented selection ordering and dynamic-tool filtering as non-parity product evidence.
- [x] Created the active ExecPlan before source implementation.
- [x] Added characterization and CLI tests for ordering, profile bounds, bare skill filtering, MCP bypass, provider-call enforcement, and zero-traffic failures.
- [x] Implemented and migrated the central resolver across interactive, headless, review, checkpoint, scripted, subagent, skill-capability, and MCP paths.
- [x] Pass focused and repository-wide gates.
- [x] Complete isolated walkthrough and harness reconciliation.

## Decisions

- Decision: use canonical snake_case registry names with case-insensitive matching, not Ante's display names or matcher syntax.
  Evidence: `ca` handlers, schemas, approvals, and provider tool calls already share exact snake_case names; matcher syntax belongs with APR-055.
  Consequence: `--tools READ_FILE` selects `read_file`, while `Bash(cargo test)` remains invalid in this slice.
- Decision: make profile eligibility an immutable upper bound and omit globally known but ineligible names per runner.
  Evidence: one run policy must span intentionally different Main, Plan, Review, Checkpoint, and Subagent profiles without widening or causing checkpoint failures.
  Consequence: a Main-oriented replacement can yield an empty checkpoint and a reduced review, never a more privileged one.
- Decision: use repeatable, comma-delimited single-value options rather than a greedy variadic parser.
  Evidence: `ca` preserves free positional prompts and `@file` attachments, while a variadic option cannot reliably distinguish the last tool from the first unflagged prompt.
  Consequence: multiple tools use `--tools read_file,search_files` or repeated `--tools`; help and tests state this explicitly.
- Decision: validate potential MCP names only against the final assembled registry.
  Evidence: MCP tools are dynamically registered and cannot be known from built-ins at CLI parse time.
  Consequence: unknown dynamic names may require local MCP catalog startup, but still fail before provider traffic.

## Surprises and Discoveries

- REQ-053's bare walkthrough captured 35 actual tools because the post-MCP request path bypassed the preview's skill-capability filter.
- Headless's per-turn MCP catalog replacement similarly discards its caller-provided skill/toolless filtering.
- Comparing actual tool names to a filtered built-in name set can misclassify a reintroduced built-in as MCP context; origin metadata is the authoritative partition.
- Existing scripted fake-backend tests had three fixtures that called `read_file` while advertising no tools. Updating the fixtures to advertise their exact intended tool restored the characterization and proved hidden calls now fail closed.
- Strict Clippy caught the review runner crossing its argument-count limit after policy injection. `ReviewRunConfig` now groups immutable review-run inputs without weakening the lint.

## Recovery and Rollback

- Code rollback is patch-only and limited to TOL-054-owned files. Do not use destructive Git commands.
- Keep `ToolRegistry::definitions` until every production caller is migrated so individual paths can be repaired without changing schemas or handlers.
- Disposable test/walkthrough roots and exact loopback PIDs are recorded before cleanup; stop only those PIDs and move only those roots to Trash.

## Completion Record

Changed files: `src/tool_selection.rs`, `src/{main,app,headless,review,test,test_tests}.rs`, `src/agent/{tools,subagent}.rs`, `src/tui/render_tests.rs`, two reviewed snapshots, `tests/cli.rs`, and the required harness/plan documents.

Commands and results: focused suites PASS; full suite PASS (635 unit + 26 CLI); formatting, strict Clippy, release build, help/version/profile/flag-order smoke, snapshots, and isolated walkthrough PASS.

Remaining risks: selection accepts exact registered names only; richer tool/path/command permission rules remain APR-055. Dynamic MCP names still require the existing catalog assembly before final validation; deferred activation remains MCP-064.

Harness updates: reconciled `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md`. This remains an Ante-informed `ca` extension, not Cursor parity.
