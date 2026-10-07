# APR-055 — Rich Permission Policy

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Add one typed permission evaluator for exact tool, canonical path-glob, and parsed simple-command-prefix rules with fixed deny/session-grant/ask/allow precedence, safe shell fallback, and a process-only `/permissions` inspection/removal manager, while retaining every existing workspace, sandbox, network, tool-selection, and subagent boundary.

## Non-Goals

- Changing tool exposure (`TOL-054` owns it), making permission imply workspace/network/sandbox authority, or allowing a rule to bypass schema/argument validation.
- General shell parsing, pipelines/redirections/expansion approval, regex matchers, arbitrary argument predicates, per-project rule files, or policy synchronization.
- Changing the existing approval mode names, making headless implicitly yolo, granting network persistently, or copying Ante's default rule table/mode behavior.
- Persisting session grants, placing rules/grants in sessions, exposing raw tool arguments, or claiming Cursor/Ante runtime parity.

## Evidence and Current Behavior

- `src/config.rs::ApprovalRule` supports only `AllowTool`, `DenyTool`, `AllowPath`, and `DenyPath` under `approval_rules`; `ConfigStore::append_approval_rule` rewrites that one array.
- `src/approval.rs::check_approval` applies deny before allow and returns only allow/deny/no-match. Path matching resolves one `path`/`cwd` value but does not share the prepared invocation's typed resource set.
- `src/main.rs` currently persists broad allow/deny rules directly from approval keys and has no process-only grant state or manager. Network approval remains intentionally distinct and one-run only.
- Headless, interactive, MCP, and provider-subagent paths all consult `ApprovalBroker`, but one interactive precheck still special-cases only `DenyTool`; this is a characterization target.
- Ante documents explicit Allow/Ask/Deny decisions, deny > session grant > ask > allow precedence, scoped temporary grants, normalized paths, and refusal to let a simple command matcher vouch for multi-stage shell input in `oracles/ante-main/docs-site/docs/configuration/permission.mdx`. This is documented product evidence only, not runtime observation or Cursor parity.
- Inferred: rich rules should live beside the legacy `approval_rules` key so loading or appending a new rule never reformats/replaces an existing user's legacy array. Both forms must participate in one evaluator.

## Scope

Allowed files:

- new `src/permission.rs`
- `src/{config,approval,app,main,headless}.rs`, `src/agent/subagent.rs`, `src/tui/{commands,input,render,render_tests}.rs`, and focused tests/snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `docs/.INDEX.md`, and this ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, provider/session/request protocols, real user config/Keychain/sessions, installed binary, external services, and unrelated future roadmap slices
- network-grant semantics, workspace-root authority, sandbox modes, tool schemas/selection, or subagent exact tool/path/command grant widening

Runtime state affected:

- Persistent rich rules are comment-preserving config entries. Existing legacy rules remain readable and are not rewritten merely by loading or adding rich rules.
- Session grants are bounded process-only safe matchers. They are neither serialized nor copied into session history/request/debug state.

## Design and Invariants

- `PermissionDecision` is `Allow`, `Ask`, or `Deny`. `PermissionMatcher` is an exact canonical tool name, a tool plus canonical absolute path glob, or a `run_command` simple-command token prefix. `PermissionRule` pairs the two with strict bounded validation.
- Existing `ApprovalRule` values are mapped in memory to equivalent typed decisions/matchers. `CaConfig` retains `approval_rules` unchanged and adds a separate defaulted `permission_rules` collection. Appending/removing a rich rule patches only that key; legacy removal, when explicitly requested, removes only the selected array element.
- Evaluation happens only after registry validation and workspace preparation. File matchers inspect prepared canonical absolute path resources, never unresolved model spelling. Invalid globs/config entries fail config load before runtime construction.
- Command prefixes contain 1–8 nonempty UTF-8 tokens with per-token/total bounds. A small deterministic lexer accepts one command with whitespace and simple single/double quoting. Shell operators, newlines, redirection, expansion, substitutions, backticks, escapes, control characters, unterminated quotes, and multiple stages are unsafe.
- A deny rule always wins. An exact process session grant wins over persistent Ask/Allow but never Deny. Without a grant, Ask wins over Allow. No match returns the existing mode/tool default. Unsafe `run_command` input can still match a broad Deny but never auto-allows from a persistent rule, session grant, auto-review, or yolo; it remains an explicit approval in TUI and an approval-required headless exit.
- Network requests retain the current distinct one-call approval after permission classification. No persistent or session rule authorizes network.
- Approving for the session derives the narrowest safe matcher: a canonical exact path for path-bearing tools, a validated two-token simple-command prefix when available, otherwise exact tool name. Unsafe shell input receives no session grant. Subsumed grants collapse deterministically under a fixed count/byte cap.
- `/permissions [filter]` is visible and idle-only. It displays safe source/decision/matcher rows for legacy, rich, and session entries; resolved arguments, file contents, endpoints, credentials, and command tails never enter manager state. `Shift+D` uses Cancel-first confirmation. Bare may remove process grants but cannot persist config changes.
- Persistent mutation uses the blocking worker, locks the manager, preserves filter/selection on failure, reloads the effective policy after success, and rejects stale generations. Removing one rule does not rewrite unrelated TOML/comments.
- Tool selection and permission remain independent. A permission can never make an unadvertised/ineligible tool callable. Existing exact subagent grants remain an upper bound, and review/checkpoint/provider-probe/BTW tool-free/read-only boundaries remain unchanged.

## Milestones

### M1 — Characterize legacy behavior and unsafe gaps

- Add failing evaluator/config tests for Ask precedence, canonical path matching, safe command prefixes, unsafe shell fallback, legacy compatibility/non-rewrite, and zero-I/O validation failures.
- Add runner tests showing interactive/headless/MCP/subagent calls use one decision and network stays distinct.
- Validation: focused tests fail only on missing rich-policy behavior.

### M2 — Implement typed policy and migrate broker consumers

- Add typed decisions/matchers, validation, legacy adaptation, session-grant store, command lexer, and one evaluator.
- Route every existing broker consumer and interactive batch preclassification through it; remove direct variant-specific checks.
- Add comment-preserving append/remove APIs and prove new saves do not rewrite legacy arrays/unrelated TOML.
- Validation: core/config/runner suites pass with no real commands, network, or user state.

### M3 — Add session grant and manager UX

- Add approval-prompt session-grant action plus `/permissions` list/filter/delete/confirmation/busy/failure state.
- Keep manager snapshots secret-free and persistent mutations asynchronous; update dynamic help and wide/narrow snapshots.
- Validation: command/input/orchestration/render suites cover empty/populated/filter/legacy/rich/session, Cancel, busy, bare, success, and failure.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, full tests, formatting, strict Clippy, release build, help/version/profile smoke, and zero-pending-snapshot checks.
- With a disposable home/workspace and loopback provider, exercise deny > ask > allow, canonical-path matching, session grant/removal, safe command prefix, unsafe command reprompt, headless failure, and unchanged network approval.
- Reconcile harness documents, archive this plan, and activate HED-056 only after every gate passes.

## Acceptance Criteria

- Typed exact-tool, canonical-path-glob, and simple-command-prefix rules load, validate, evaluate, append, and remove deterministically.
- Precedence is deny > session grant > ask > allow > existing fallback; network remains a separate prompt and unsafe shell syntax cannot auto-execute.
- Legacy configs load unchanged, participate equivalently, and are not rewritten when rich rules are added or session grants change.
- Canonically equivalent in-root paths match the same rule; traversal/outside-root calls fail before policy and symlink/canonicalization cannot bypass a deny.
- Interactive, headless, MCP, and subagent tool calls use the same policy boundary; reviews/checkpoints/probes/BTW retain their existing stricter behavior.
- Session grants are bounded, scoped, inspectable/removable, process-only, and absent from config, requests, sessions, snapshots, and debug output.
- `/permissions` is safe, filterable, Cancel-first for deletion, mutation-locked, bare-aware, and failure-preserving at wide/narrow sizes.
- Focused/full gates and isolated walkthrough pass without touching real runtime state, oracle files, external services, or the installed binary.

## Validation

Focused:

- PASS: typed policy/config/broker tests cover precedence, legacy adaptation, canonical paths and symlinks, safe prefixes, unsafe-shell fallback, bounded session grants, comment-preserving mutations, interactive/headless/MCP/subagent routing, and no-side-effect yolo behavior.
- PASS: `/permissions` command, input, orchestration, bare, stale-generation, and failure-recovery tests.
- PASS: ten reviewed wide/narrow populated, empty, confirmation, busy, and failure snapshots; zero pending `.snap.new` files.

Completion:

- `cargo test --no-fail-fast`: PASS, 657 unit tests plus 26 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Rebuilt help/version/lazy-profile/headless smoke: PASS.

Manual/visual:

- PASS with a disposable home/workspace and credentialless loopback provider. The walkthrough exercised Ask over broad Allow, approving a narrow process-only session grant, reusing and removing that grant, default-Cancel deletion, renewed prompting after removal, unsafe-shell prompting under allow-all, separate network approval, and headless unsafe-shell refusal.
- Neither denied output file existed, no session was saved under `--no-session`, no session grant entered TOML, and both exact temporary processes stopped. The disposable root was moved recoverably to `/Users/aji/.Trash/ca-apr055-walk.EAQDJy`.

Blocked checks:

- None currently.

## Progress

- [x] Inspected the current config schema, broker precedence, interactive prompt actions, headless/MCP/subagent consumers, and prepared workspace path boundary.
- [x] Reviewed Ante's documented permission grammar as non-parity product evidence.
- [x] Created the active ExecPlan before source implementation.
- [x] Add failing core/config/runner characterizations.
- [x] Implement typed evaluation and migrate broker consumers.
- [x] Add session grants and `/permissions` manager.
- [x] Pass focused and repository-wide gates.
- [x] Complete isolated walkthrough and harness reconciliation.

## Decisions

- Decision: keep `approval_rules` as the legacy wire format and add rich `permission_rules` rather than migrating on load.
  Evidence: existing user files use externally tagged enum values and config mutation is comment-preserving only when the exact key is patched.
  Consequence: one evaluator adapts both lists, while adding a rich rule cannot rewrite legacy syntax.
- Decision: treat unsafe shell as always Ask except for a matching broad Deny.
  Evidence: a prefix cannot prove the safety of pipelines, redirections, substitution, expansion, or multiple commands.
  Consequence: TUI users may approve the exact call once, but it is never auto-approved or session-granted; headless reports approval required.
- Decision: keep network authorization separate and non-persistable.
  Evidence: current architecture requires explicit run/server network grants even when tool approval/yolo is present.
  Consequence: permission rules only answer tool execution policy and cannot satisfy network authority.

## Surprises and Discoveries

- The interactive batch path has a direct `DenyTool` precheck before the broker, so rich/path denies would currently take a different route unless that special case is removed.
- Prepared built-in arguments already contain canonical absolute paths, giving rich path rules a stronger match boundary than the legacy raw `path`/`cwd` fallback.
- The current approval footer has persistent broad tool/path actions but no temporary scoped grant or inspection surface.
- Foreground approval persistence previously bypassed the blocking mutation worker; rich rule saves now keep the prompt open and report an inline error unless the atomic patch succeeds.
- Manager rows must abbreviate command prefixes to the first two tokens so policy inspection never becomes a command-argument history surface.
- A disposable loopback fixture initially reused one tool-call ID across turns; restarting it with monotonically unique IDs isolated that fixture defect from the verified session-grant behavior.

## Recovery and Rollback

- Code rollback is patch-only and limited to APR-055-owned files. Do not use destructive Git commands.
- Keep legacy deserialization/evaluation active throughout migration so a partial implementation cannot strand existing configs.
- Disposable walkthrough processes are tracked by exact PID; stop only those processes and move only the exact temporary root to Trash.

## Completion Record

Changed files: `src/permission.rs`, `src/{config,approval,app,main,headless}.rs`, `src/agent/subagent.rs`, `src/tui/{commands,input,render_tests}.rs`, ten permission-manager snapshots, and the repository harness documents listed in Scope.

Commands and results: focused policy/config/runner/manager suites passed; `cargo test --no-fail-fast` passed 657 unit plus 26 CLI tests; formatting, strict Clippy, release build, help/version/profile/headless smoke, zero-pending-snapshot check, and the isolated TUI walkthrough all passed.

Remaining risks: rich command-prefix matching is deliberately not a general shell parser; unsafe syntax always returns to explicit approval unless denied. Network grants remain separate and process-only session grants disappear at exit by design.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, and `docs/.INDEX.md` now record the typed policy boundary, validation evidence, extension status, and HED-056 handoff.
