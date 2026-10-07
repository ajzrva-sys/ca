# LRN-176 — Foreign Knowledge Interoperability

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-25

## Goal

CA automatically discovers and can use compatible Codex, Hermes, and MiMo
skills, and manual learning can consult their bounded read-only memories and
lessons with explicit source provenance.

## Non-Goals

- Copying or modifying oracle implementations.
- Mutating foreign skill, command, memory, trajectory, or database stores.
- Treating foreign text as instructions or granting authority from foreign
  tool/model/dependency declarations.
- Importing credentials, raw tool outputs, reasoning, email, incomplete turns,
  or subagent chatter.
- Installing dependencies or executing foreign scripts during discovery or
  learning.

## Evidence and Current Behavior

- `src/main.rs::discover_skill_registry` discovers CA, Agent Skills, admin,
  managed, auto-distilled, and explicitly configured compatibility roots, but
  does not automatically mount Codex, Hermes, or MiMo roots.
- `src/learning.rs::existing_asset_catalog` inventories only CA/Agent Skills
  roots plus native prompt commands and agents.
- `src/codex_learning.rs` already indexes native CA sessions, Codex archives,
  and the MiMo trajectory database read-only, but not the systems' reusable
  skill or memory stores.
- `src/skills.rs::SkillRegistry::discover_portable` already validates bounded
  complete `SKILL.md` trees and keeps `Managed` sources read-only.
- Read-only oracle evidence shows MiMo treats raw trajectory as authoritative
  and memory as an index while inventorying existing skills, commands, and
  agents before distillation. No oracle source will be copied.
- The local Hermes installation contains portable `SKILL.md` trees under
  `~/.hermes/skills`; exact memory storage still requires bounded
  characterization before implementation.

## Scope

Allowed files:

- `src/{runtime,skills,main,learning,codex_learning,prompt_commands}.rs`
- `src/{foreign_knowledge,memory}.rs`
- focused tests colocated with those modules
- `ARCHITECTURE.md`, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
  `docs/.INDEX.md`, and this plan

Forbidden files/areas:

- `oracles/`, `reference/`, and all foreign runtime stores
- provider credentials and authentication files
- installation of `target/release/ca`

Runtime state affected: none during implementation and validation. Discovery
is read-only. No manual learning run is authorized by this plan.

## Design and Invariants

- Foreign portable skills are mounted directly from stable roots as read-only
  `Managed` instances. Normal name ambiguity, exact selectors, structural
  validation, activation budgets, and ordinary runtime approvals still apply.
- Workspace-local foreign roots precede profile/global foreign roots; CA-owned
  roots retain their existing precedence. Built-in commands cannot be
  shadowed.
- Foreign memories and lessons are untrusted evidence, never system
  instructions. A host adapter emits bounded sanitized projections with
  source, scope, stable identifier, digest, title/kind, and text.
- Raw foreign text remains ephemeral. CA persists only existing learning
  coverage metadata and publication journals.
- SQLite sources are opened query-only and must not create WAL/SHM files.
- One unavailable or malformed foreign source degrades independently and does
  not stop CA or discard other sources.

## Milestones

### M1 — Characterize and lock the boundary

- Add fixtures for automatic foreign skill roots and bounded foreign knowledge
  discovery.
- Record exact supported roots/formats and rejection behavior.
- Validation: focused failing tests.

### M2 — Runtime skill interoperability

- Add deterministic built-in discovery roots for Codex, Hermes, and MiMo/
  OpenCode-compatible portable skills.
- Keep foreign trees read-only and expose unambiguous instances through the
  existing slash-skill path.
- Validation: focused skills/main/prompt-command tests.

### M3 — Learning knowledge interoperability

- Build bounded, sanitized, read-only foreign knowledge projections and add
  them to the manual learning inventory/search protocol without treating them
  as recurring-session support.
- Inventory foreign reusable assets before publication so CA extends or skips
  rather than duplicates them.
- Validation: focused learning tests for provenance, bounds, hostile text,
  privacy exclusions, unavailable sources, and read-only SQLite behavior.

### M4 — Completion gates and durable handoff

- Run focused suites and the repository completion gates.
- Update architecture, testing, continuity, and next-step records, then move
  this plan to completed only if all acceptance criteria pass.

## Acceptance Criteria

- A compatible skill placed only in a Codex, Hermes, or MiMo supported root is
  discovered, selectable, and usable through CA without copying it.
- Foreign skill trees cannot be modified by `skill_manage`; authority remains
  ordinary CA runtime authority.
- Manual learning's inventory names the foreign source and existing asset;
  bounded search can retrieve a sanitized foreign lesson/memory projection.
- Hostile embedded instructions, links, secrets, raw outputs, and unsupported
  database fields are absent from model-visible projections.
- Missing/malformed foreign stores do not prevent CA startup or learning.
- No foreign source file or database changes during tests.

## Validation

Focused:

- `cargo test skills::tests`
- targeted `main`, `prompt_commands`, `learning`, and `codex_learning` tests

Completion:

- `cargo test`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

Manual/visual: no runtime installation or real `/learn` invocation is planned.

Blocked checks: network/provider validation is unnecessary for the read-only
host boundary and must not be claimed.

## Progress

- [x] Current skill registry and learning inventory traced.
- [x] Exact foreign roots and memory formats characterized.
- [x] Runtime foreign skill discovery implemented and tested.
- [x] Bounded foreign knowledge ingestion implemented and tested.
- [x] Completion gates and harness reconciliation recorded.

## Decisions

- Decision: mount compatible skills instead of copying them.
  Evidence: CA already validates complete portable trees and read-only managed
  instances.
  Consequence: foreign updates are visible after registry refresh, provenance
  stays exact, and CA never owns the source tree.

- Decision: foreign memories are evidence, not instruction context.
  Evidence: the stores can contain untrusted external content and provider-
  specific conventions.
  Consequence: only host-sanitized bounded projections reach the learning
  agent, and they cannot grant permissions or independently prove recurrence.

## Surprises and Discoveries

- Hermes's `~/.hermes/skills` is categorized one level deeper than the common
  flat Agent Skills layout and uses descriptive `version`, `author`,
  `platforms`, `prerequisites`, and related frontmatter. CA's recursive walker
  already handled the layout; the parser now accepts these fields inertly.
- The first real registry check mounted Codex `.system` twice through an
  overlapping explicit root. Removing that overlap leaves one exact immutable
  instance instead of an ambiguous duplicate.
- The existing machine exposes 81 structurally compatible Hermes skills, 22
  bundled MiMo skills, and one Codex skill. The sole rejected Hermes tree
  contains binary PDF templates, which violates CA's text-only portable-tree
  invariant; the same named skill is available from the MiMo bundle.
- Hermes's `state.db` distinguishes main CLI and subagent sessions. The adapter
  filters subagents at the session query and never selects reasoning columns.

## Recovery and Rollback

Remove the added discovery adapters and roots with `apply_patch`; no foreign or
user runtime state is mutated, so no data rollback is required. Preserve all
unrelated dirty-worktree changes.

## Completion Record

Changed files: `src/foreign_knowledge.rs`, `src/{main,memory,skills,
prompt_commands,learning,codex_learning}.rs`, `ARCHITECTURE.md`, `TESTING.md`,
`CONTINUITY.md`, `NEXT_STEPS.md`, `docs/.INDEX.md`, and this plan.

Behavior delivered:

- Compatible Codex, Hermes, and MiMo skills are mounted in place as immutable
  `Managed` instances and immediately enter CA's normal skill/slash flow.
- MiMo/OpenCode/Codex-compatible prompt commands are ordinary inert prompt
  turns with literal arguments and built-in collision protection.
- Bounded Codex/Hermes/MiMo memory notes participate in query recall and direct
  learning with explicit untrusted provenance.
- Hermes completed main-session history is a standard query-only learning
  source with safe action outcome/recovery projection and full privacy
  exclusions.

Commands and results:

- Foreign knowledge tests: 3 passed.
- Prompt command tests: 5 passed.
- Memory tests: 13 passed.
- Skill tests: 21 passed.
- Archive learning tests: 65 passed.
- Real read-only `ca skill list`: 81 Hermes, 22 MiMo, one Codex instance; no
  duplicated Codex system skill.
- Real local-only Hermes source preview: one file, four completed candidate
  messages, zero provider calls, zero profile writes.
- `cargo test`: 2,593 passed, 3 ignored.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo build --release`: passed.

Remaining risk: a foreign tree containing binary or linked resources remains
unavailable as a complete CA skill tree by design. CA reports/skips it rather
than weakening full-tree integrity. No provider behavior was required or
claimed.

Harness updates: architecture, testing, continuity, next steps, and repository
index now record LRN-176 as complete. No `/learn` run, runtime publication,
foreign write, release installation, or provider call occurred.
