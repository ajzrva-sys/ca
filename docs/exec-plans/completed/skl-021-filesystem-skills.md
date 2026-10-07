# SKL-021 — Filesystem `SKILL.md` Registry

Status: COMPLETE
Owner/session: Codex primary task
Last updated: 2026-08-09

## Goal

Discover validated project/user skills without following symlinks outside their roots, expose only bounded metadata until exact activation, and persist hash-bound active references across sessions.

## Non-Goals

- Remote installation, downloads, automatic relevance activation, or dynamic slash commands.
- Compatibility roots unless explicitly configured.
- Model-visible secret input or expanded authority from skill text.
- Cursor parity claims while `PAR-005` is blocked.
- Oracle/reference edits, runtime credential changes, installed-binary changes, or live-TUI interaction.

## Evidence and Current Behavior

- No production skill module or config exists.
- `build_system_prompt` currently combines base and project instructions only; it has no bounded metadata/body boundary.
- `SessionV2` has optional workspace/reasoning/checkpoint additions but no active external-content references.
- `RawCliArgs` has no repeatable `--skill`; headless builds its system prompt before the shared runner.
- The command registry and pager provide an existing searchable/list surface pattern.
- The typed tool registry has no exact skill-activation tool.
- `RuntimePaths::config_root` and `WorkspaceContext::primary` provide injected user/project roots.
- `sha2` is not currently a dependency. Inferred: a small strict frontmatter parser avoids a broad YAML parser while supporting the required three fields.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/`, focused tests/snapshots, this plan, `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md`.

Forbidden: `oracles/`, `reference/`, credentials, user runtime config/sessions, installed binary, live processes outside hermetic tests.

Runtime state: automated tests use injected temporary project/user roots only.

## Design and Invariants

- `SkillRegistry` discovers exactly `**/SKILL.md` beneath project `.ca/skills`, user `.config/ca/skills`, and explicitly configured compatibility roots.
- Discovery does not traverse symlink directories/files; canonical files must remain beneath their declared root.
- Files over 128 KiB, malformed/unknown frontmatter, invalid IDs/aliases, duplicate IDs/aliases, and duplicate canonical files are rejected with deterministic diagnostics.
- Precedence is project over user over compatibility. Same-tier collisions are rejected; lower-tier collisions are excluded with deterministic shadowing diagnostics.
- Catalog entries include bounded ID/name/description/aliases/origin only; body text is loaded only by exact user `--skill`, `/skills activate`, or the exact-ID activation tool after normal approval policy.
- Active skill bodies total at most 12,000 estimated tokens and are included as clearly delimited instructions without changing tool/workspace authority.
- Persisted references contain only ID, canonical path, and SHA-256. Missing/hash-changed references are inactive on restore with visible warnings.
- Headless `--skill <id>` is repeatable and resolves before provider I/O; invalid activation is a semantic input error.
- `/skills` is searchable through arguments and supports list/activate/deactivate/view/reload. Full bodies appear only in explicit view or active prompt assembly.

## Milestones

### M1 — Strict discovery and validation
- Add failing temporary-root tests for precedence, strict frontmatter, size, duplicates, symlink escape, hashing, and compatibility-root opt-in.
- Implement `SkillRegistry`, metadata, diagnostics, and injected roots.

### M2 — Activation, budgets, and prompt boundary
- Add failing tests for exact ID/alias activation, disabled config, 12,000-token budget, bounded model catalog, and body inclusion only after activation.
- Implement activation/deactivation/reload and system-prompt assembly.

### M3 — CLI, command, tool, and persistence
- Add repeatable `--skill`, `/skills`, exact-ID activation tool, active-reference save/restore, changed/missing warnings, and branch retention.
- Add focused subprocess, registry, session, orchestration, and wide/narrow render tests.

### M4 — Completion
- Run focused suites and full format/Clippy/test/release/help/version gates.
- Reconcile harness docs and move this plan only after every gate passes.

## Acceptance Criteria

- Project/user precedence and explicit compatibility roots are deterministic.
- Symlink escapes, oversize files, malformed/unknown frontmatter, invalid IDs, and ID/alias collisions never enter the catalog.
- Metadata is bounded and skill bodies are absent from provider prompts until exact activation.
- Active content cannot exceed the token budget.
- CLI/TUI/tool activation resolves exact identities; no fuzzy or relevance auto-activation occurs.
- Session refs restore only when path and SHA match; forks retain refs and warnings are visible.
- All tests are temporary-state-only and completion gates pass.

## Validation

Focused: PASS — skills (7), typed tool activation, headless next-turn prompt, CLI subprocess, config/runtime, session, restore/save, command/orchestration, and render suites.
Completion: PASS — `cargo test` (243 unit + 16 CLI), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, local help/version.
Manual/visual: reviewed `/skills` catalog at 100×30 and 60×24; 50 accepted snapshots and zero `.snap.new`. The live TUI was not touched.
Blocked checks: clean Cursor comparison remains blocked under `PAR-005`.

## Progress

- [x] Current config/runtime/workspace/prompt/CLI/command/tool/session boundaries traced.
- [x] Characterization tests added.
- [x] Discovery and validation complete.
- [x] Activation/prompt boundary complete.
- [x] CLI/TUI/tool/persistence complete.
- [x] Completion gates and documentation complete.

## Decisions

- Decision: project/user/compatibility precedence resolves cross-tier collisions; same-tier collisions are rejected.
  Evidence: validation requires both precedence and duplicate rejection.
  Consequence: local project intent wins predictably while no ambiguous identity exists within one authority tier.
- Decision: use a strict field-limited frontmatter parser.
  Evidence: only name, description, and aliases are supported and unknown fields must fail.
  Consequence: no permissive YAML behavior or extra parser surface enters this task.

## Surprises and Discoveries

- The exact user and project roots already exist as injected boundaries, so skill tests can remain independent of real HOME/workspace state.
- Session restore must rediscover the project catalog under the restored workspace primary before validating active references; retaining the launch catalog produces false missing warnings.
- A symlinked `.ca` directory can escape even when the final `skills` component is not itself a symlink, so project discovery also checks the canonical catalog root against the workspace boundary.

## Recovery and Rollback

Use milestone-scoped `apply_patch` reversals; do not use destructive Git commands. New config/session fields remain optional/defaulted.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/skills.rs`, `src/{main.rs,app.rs,config.rs,runtime.rs,session.rs,headless.rs}`, `src/agent/{registry.rs,tools.rs}`, `src/tui/{commands.rs,render_tests.rs,snapshots/*skills_catalog*.snap}`, `tests/cli.rs`, and harness documentation.

Commands and results: focused suites PASS; `cargo test` PASS (243 + 16); format PASS; strict Clippy PASS; release PASS; local help/version PASS.

Remaining risks: strict frontmatter intentionally supports only scalar name/description plus inline/block alias lists, not general YAML. No live interactive TUI or external provider was exercised. Cursor parity remains unverified.

Harness updates: `NEXT_STEPS.md`, `TESTING.md`, `CONTINUITY.md`, and `ARCHITECTURE.md` reconciled; this plan moved to completed.
