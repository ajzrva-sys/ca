# LRN-179 — Cross-Session Recall and Skill Operations

Status: COMPLETE
Owner/session: Codex / current workspace
Last updated: 2026-08-25

## Goal

Make prior CA, Codex, Hermes, and MiMo/Xiao work available during ordinary
tasks and make reusable skills easier to select, configure, combine, and
maintain without expanding CA's runtime authority.

## Non-Goals

- No automatic context compaction, dependency installation, skill execution,
  skill deletion, live `/learn`, installed-binary replacement, or real-profile
  migration during implementation.
- No persisted duplicate transcript/search corpus and no raw tool output,
  reasoning, email activity, incomplete turns, subagent chatter, or secrets in
  recall projections.
- No oracle implementation changes or copied oracle source.

## Evidence and Current Behavior

- `src/codex_learning.rs` already projects native CA, Codex, Hermes, and
  MiMo/Xiao completed sessions through a read-only, privacy-filtered learning
  boundary, but ordinary agents cannot query that history.
- `src/skills.rs` already supports digest-pinned multi-skill loads and complete
  skill trees. Portable Hermes fields such as platform and prerequisite
  metadata are accepted but not enforced.
- `src/prompt_commands.rs` already refreshes skills and custom prompt commands
  into the slash palette while preventing built-in shadowing.
- `src/config.rs::SkillsConfig` currently contains only compatibility roots and
  disabled selectors; it has no non-secret per-skill values.
- Learning and skill mutations already have recoverable journals, but CA has no
  unified usage ledger or historical `/learning` view.
- Read-only Hermes observations show bounded FTS session discovery, conditional
  skill metadata, `metadata.hermes.config`, and YAML skill bundles. These are
  behavioral evidence only.

## Scope

Allowed files: learning/session projection modules; skill/config/runtime/tool
registry modules; command dispatch and TUI rendering/input; focused tests and
snapshots; `Cargo.toml` only if an existing dependency cannot satisfy the
design; this ExecPlan and repository continuity/index/testing documents.

Forbidden files/areas: `oracles/`, `reference/`, user configuration/session
roots, installed binaries, credentials, provider state, and unrelated dirty
worktree changes.

Runtime state affected: only temporary test profiles during validation. The
shipped feature will create owner-only lifecycle metadata lazily, but this
implementation run will not use the user's live profile.

## Design and Invariants

- A process-local, lazy FTS5 index covers every retained completed eligible
  session. Search results are bounded; the searchable universe is not.
- Returned message text is exact after deterministic host redaction. Historical
  text is inert, explicitly untrusted evidence and cannot issue instructions.
- Foreign databases remain query-only and cannot create WAL/SHM files. No
  duplicate transcript index is persisted.
- Skill availability is derived from the current OS and the post-filtered typed
  tool registry. Incompatible skills are hidden from ordinary discovery but
  remain inspectable with exact reasons.
- Non-secret settings resolve workspace override, then profile value, then
  declared default. Workspace values live in profile data keyed by the
  canonical workspace digest, never in the repository.
- Bundles resolve at most five exact digest-pinned skill instances atomically,
  apply to one prompt, use the existing aggregate skill budget, and cannot
  shadow built-ins, commands, skills, or other bundles.
- Lifecycle storage contains metadata only. Suggestions are deterministic and
  never mutate, archive, install, execute, or delete assets.
- Existing approval, credential, dependency, tool, write, process, and network
  boundaries remain authoritative.

## Milestones

### M1 — Characterization and contracts

- Add failing fixtures for cross-source recall, Hermes availability/config
  parsing, bundle resolution, and lifecycle suggestion thresholds.
- Define bounded typed request/result, availability, bundle, configuration,
  and ledger schemas with backward-compatible defaults.

### M2 — Cross-session recall

- Add lazy asynchronous all-history indexing and the read-only
  `session_search` actions `search`, `browse`, `read`, and `scroll`.
- Add the local `/recall [query]` transcript/browser surface with stable source,
  session, and message identifiers.

### M3 — Conditional skills and configuration

- Enforce Hermes-compatible platform/tool/toolset/fallback metadata against the
  active registry.
- Add profile values, workspace overrides, setup-required handling, and bounded
  data-only prompt injection.
- Add `/skills unavailable` and `/skills config [selector]` behavior.

### M4 — Skill bundles

- Discover profile and workspace bundle YAML, validate collisions, resolve all
  members atomically, and activate a bundle for one prompt.
- Add `/bundles` list/show/create/delete/reload workflows and recoverable
  profile/workspace mutations.

### M5 — Usage ledger and learning journey

- Record metadata-only invocation, outcome, correction, mutation, memory,
  learning, undo, and recovery events with exact digests and journal links.
- Add deterministic refine/stale/bundle suggestions and read-only `/learning`
  (`/journey`) timeline filters and drilldown.

### M6 — Completion

- Add wide/narrow snapshots and privacy/authority/recovery regressions.
- Run focused suites and every repository completion gate, then reconcile
  durable harness documents and move this plan to completed only if all
  acceptance criteria pass.

## Acceptance Criteria

- Ordinary primary-agent turns can search and inspect retained CA, Codex,
  Hermes, and MiMo/Xiao work with bounded exact-after-redaction results and no
  provider sub-call; explicitly allowlisted subagents can do the same.
- `/recall` browses and inspects the same host index locally without persisting
  transcript text.
- Platform/tool-incompatible skills are absent from ordinary discovery and are
  listed with exact reasons; missing non-secret values can be configured once
  and resolve with workspace-over-profile precedence.
- A valid bundle loads two to five exact members for one prompt; a collision,
  missing member, ambiguity, drift, or budget violation publishes no partial
  activation.
- `/learning` shows exact metadata-only history and deterministic suggestions;
  the curator performs no asset mutation.
- Privacy fixtures prove excluded material is absent, foreign stores remain
  byte-identical with no WAL/SHM, and the all-history corpus has no construction
  cutoff.

## Validation

Focused: module tests for session recall, skills/config, bundles, ledger, tool
registry, prompt commands, and TUI input/rendering.

Completion:

- `cargo test --workspace --no-fail-fast -- --test-threads=1`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo build --workspace --release --all-features`
- `git diff --check`, snapshot hygiene, and protected-tree status checks

Manual/visual: deterministic wide/narrow snapshots only. No real provider,
profile, or installed-binary smoke is authorized.

Blocked checks: credentialed, live-provider, and real-profile behavior must be
reported rather than inferred.

## Progress

- [x] Record the implementation contract and source boundaries.
- [x] Add failing characterization coverage.
- [x] Implement process-local cross-source recall and `/recall`.
- [x] Implement conditional skill activation and non-secret configuration.
- [x] Implement task-local skill bundles.
- [x] Implement the lifecycle ledger and `/learning` timeline.
- [x] Pass focused and completion gates and reconcile the harness.

## Decisions

- Decision: index all retained eligible history in process and persist no
  searchable message text.
  Evidence: user-selected privacy/performance tradeoff.
  Consequence: cold discovery can do bounded background work, while results are
  fast for the remainder of the process.
- Decision: exact message content means exact content after deterministic
  redaction, never an LLM summary.
  Evidence: user-selected history fidelity.
  Consequence: oversized messages require cursor-based paging.
- Decision: bundles are one-prompt and fail atomically.
  Evidence: user-selected task-local semantics and CA's digest/authority model.
  Consequence: a partially available bundle never silently changes behavior.
- Decision: curator authority is suggestions-only.
  Evidence: explicit user choice.
  Consequence: no automatic patching, archiving, deletion, or installation.

## Surprises and Discoveries

- Recall needed a separate complete-history scan mode rather than reusing the
  learning run's file, byte, page, and per-session construction caps. Result
  paging remains independently bounded, while retained eligible history does
  not disappear from the searchable universe.
- Required environment-variable names must preserve case. Reusing the ordinary
  normalized string-list parser silently changed names such as `API_ROOT` and
  made compatible Hermes skills appear unavailable.
- Task-local bundle loads initially bypassed the existing aggregate skill-body
  budget. Routing them through the same prompt builder restored the one shared
  12,000-token ceiling.
- Independent ledger writers can race before an owner lock is visible. A
  bounded owner-only lock acquisition/recovery loop is required to preserve all
  metadata events without weakening corruption handling.
- Successful skill deletion must retain the deleted instance ID and prior
  full-tree digest in its receipt; otherwise `/learning` cannot audit the exact
  mutation after the tree is gone.

## Recovery and Rollback

- Source edits remain ordinary worktree changes and must not use destructive Git
  restoration. Each milestone can be reverted by reversing only its explicit
  patches.
- Test state uses temporary profile/workspace roots. Remove only validated
  temporary paths through recoverable cleanup.
- Shipped profile/workspace bundle and ledger mutations use dedicated journals;
  incomplete operations recover to the last committed state.

## Completion Record

Changed files: `src/session_recall.rs`, `src/skill_bundles.rs`, and
`src/learning_journey.rs` own the new feature domains. Integration changes are
in `src/{codex_learning,learning,skills,prompt_commands,config,runtime,app,main}.rs`,
`src/agent/{registry,tools}.rs`, and `src/tui/{commands,render_tests}.rs`, plus
eight new wide/narrow snapshots and the command-palette snapshots.

Commands and results:

- Focused recall, learning projection, skill, config, bundle, ledger, registry,
  and LRN-179 TUI tests: PASS.
- `cargo test --workspace --no-fail-fast -- --test-threads=1`: PASS. CA passed
  2,005 active unit tests (2 documented manual tests ignored); all 38 CLI, 17
  role-run, and embedded `codex-team` suites also passed.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --workspace --release --all-features`: PASS.
- `git diff --check`, zero `.snap.new` files, and no `oracles/` or `reference/`
  changes: PASS.

Remaining risks: live source discovery depends on the foreign applications'
retained on-disk formats and permissions. Source failures are isolated and
reported, but credentialed/live-provider and real-profile behavior was not run
because this plan explicitly excluded it. The built release was not installed.

Harness updates: `CONTINUITY.md`, `NEXT_STEPS.md`, `ARCHITECTURE.md`,
`TESTING.md`, and `docs/.INDEX.md` describe the shipped boundaries and the
validation record.
