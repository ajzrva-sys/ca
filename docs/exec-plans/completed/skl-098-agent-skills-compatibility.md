# SKL-098 — Agent Skills Compatibility

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-13

## Goal

Add portable Agent Skills discovery and identity, progressive task-local loading, bounded resource reads, digest-pinned persistence, and safe offline management/install flows while preserving ca's existing approval, path, network, MCP, session, plugin, and execution authority.

## Non-Goals

- Granting authority from skill instructions, `allowed-tools`, scripts, dependencies, metadata, or installation source.
- Executing scripts while discovering, listing, viewing, importing, or installing a skill.
- Ambient skills in bare, scheduled, gateway, review, checkpoint, or other restrictive runners.
- Sharing mutable task-local loads across service requests, sessions, subagents, or turns after a terminal/reset boundary.
- Reopening RND-097, replacing WSP-096, changing provider behavior, or claiming Cursor parity.
- Live remote installs, real user skill imports, Keychain access, provider traffic, binary installation, or oracle/reference mutation during automated validation.

## Evidence and Current Behavior

- `src/skills.rs` owns legacy `SkillOrigin`, `SkillRoots`, `SkillMetadata`, `SkillSuggestion`, `ActiveSkillRef`, and `SkillRegistry`. Discovery currently selects one identity by project/user/compatibility precedence, rejects symlinks, hashes only `SKILL.md`, and parses only name/description/aliases.
- `activate_exact` reloads `SKILL.md`, checks the old digest, and places active bodies in shared `Arc<Mutex<SkillRegistry>>`; `/skills`, repeatable `--skill`, model `activate_skill`, session restore, bare exclusion, and plugin roots depend on this API.
- OpenAI's 2026-08-13 Build Skills documentation specifies repository ancestor `.agents/skills`, user `~/.agents/skills`, admin `/etc/codex/skills`, bundled roots, distinct duplicate names, symlinked skill folders, progressive metadata/instruction/resource loading, `$` explicit invocation, and optional `agents/openai.yaml` policy/dependencies.
- The Agent Skills specification requires YAML frontmatter, 1–64 character portable names, 1–1024 character descriptions, optional license/compatibility/metadata/allowed-tools, and conventional scripts/references/assets resources.
- Hermes documents metadata-first `skills_list`, instruction/resource `skill_view`, qualified plugin skills, CLI/TUI management, and support-file loading. Its ambient enablement and script/config behavior do not override ca authority.
- Baseline 2026-08-13: `cargo test --bin ca skills::tests --no-fail-fast` passed 11 tests; `cargo fmt --check`, snapshot hygiene, and the canonical RND route sentinel preflight passed.

## Scope

Allowed behavioral files by packet:

- P1: `src/skills.rs`, `src/runtime.rs`, `src/workspace.rs`.
- P2: `src/skills.rs`, dedicated fixtures/tests, and `Cargo.toml`/`Cargo.lock` only if a bounded parser dependency is justified.
- P3: `src/skills.rs`, `src/agent/registry.rs`, `src/agent/tools.rs`, `src/tool_selection.rs`.
- P4: `src/app.rs`, `src/main.rs`, `src/headless.rs`, `src/service.rs`, `src/agent/subagent.rs`.
- P5: skill resolution/send orchestration and focused render/status tests.
- P6: `src/session.rs`, `src/config.rs`, and exact restore/save/portable callers.
- P7: new `src/skill_cli.rs` plus narrow leading dispatch and CLI tests.
- P8: owner-only profile enable/disable store in the narrowest runtime/config boundary.
- P9: narrowly scoped skill catalog/package code reusing `src/plugin_catalog.rs` and `src/plugin.rs` primitives where safe.
- P10: `src/tui/{commands,input,render,render_tests}.rs`, focused orchestration tests, and exact skill snapshots.
- P11: `src/skills.rs` and non-interactive resolver callers/tests only.
- P12: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `TESTING.md`, `docs/.INDEX.md`, this plan, and required released CLI fixtures.

Touched-file manifest: this plan, `NEXT_STEPS.md`, `Cargo.toml`, `Cargo.lock`, `src/skills.rs`, `src/runtime.rs`, `src/workspace.rs`, `src/agent/registry.rs`, `src/agent/tools.rs`, `src/agent/subagent.rs`, `src/tool_selection.rs`, `src/main.rs`, `src/app.rs`, `src/service.rs`, `src/skill_cli.rs`, `src/session.rs`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, and `docs/.INDEX.md`. Add every later touched path here before its first edit.

Forbidden: `oracles/`, `reference/`, unrelated feature code, real credentials/secrets, live provider/mailbox traffic, user sessions, automatic installation/enablement, commits before completion, or destructive Git commands.

Runtime state affected: profile-scoped skill enable/disable and manager-installed bytes only after explicit manager confirmation. Tests and walkthroughs use disposable homes. Task loads remain process-only.

## Design and Invariants

- Preserve every valid installation as a distinct immutable `SkillInstance`; unqualified collisions return deterministic qualified choices and never select by precedence.
- Qualified names are `<scope>::<percent-encoded-relative-locator>::<declared-name>`. Instance IDs hash a domain separator, qualified name, and complete content digest. Prompt metadata never contains raw HOME paths.
- Complete digests cover normalized relative paths, permitted file type/mode, and bytes under `SKILL.md`, `references/`, `scripts/`, `assets/`, and optional `agents/openai.yaml`, with strict file/depth/per-file/aggregate bounds.
- Symlinked skill directories are accepted only when pinned beneath another declared root or an explicitly confirmed source. Resource links remain under the resolved skill root and are re-opened/rehashed at use time.
- Initial prompts contain enabled metadata only, capped at min(8,000 estimated tokens, 2% selected model context), with deterministic truncation and omitted count.
- `SkillLoadSet` belongs to one task/run. It survives exact continuations, tool calls, transport retry, and exact fallback; it clears at every success/failure/cancel/new-task/session/worktree boundary. Subagents receive immutable snapshots.
- `$qualified-name` loads explicitly before provider traffic; `$name` works only if unique. Implicit selection honors `allow_implicit_invocation: false`. Existing persistent activation remains explicit and digest-pinned.
- `skill_view(instance_id, relative_path)` is read-only and validates identity, full digest, containment, regular type, and bounds every time. Reading scripts never executes them or changes grants.
- Old `ActiveSkillRef { id, path, sha256 }` remains readable and resolves only by exact canonical path plus old `SKILL.md` digest. New refs are defaulted/versioned. Reads never rewrite old config/session bytes.
- Existing unqualified disabled names fail closed for all matching instances. Exact profile records are separate. Portable exports omit refs, task loads, bodies, and resources.
- Manager `list/show` are offline. Install/import/new use preview plus exact digest confirmation, create-new private staging, complete verification, atomic publication, no overwrite, and disabled-by-default publication.
- Scheduled/gateway consumers may use only pure `(instance_id, full_digest)` resolution; ambient names and interactive roots are unavailable.

## Milestones

### M1 — Packets 1–2: identity, roots, parser, and complete digest
- Add failing standard-root/ancestor/no-bleed/duplicate/qualification tests.
- Add failing Agent Skills metadata, inventory, bounds, symlink, and full-digest drift tests.
- Implement public scopes/instances/loads and deterministic discovery without losing legacy compatibility.
- Focused validation: `cargo test --bin ca skills::tests --no-fail-fast` and runtime/workspace slices.

### M2 — Packet 3: read-only `skill_view`
- Characterize registry schema/selection/handler availability and prove traversal, drift, oversize, script-no-exec, and no authority widening.
- Implement exact instance/resource reads through the central typed tool boundary.

### M3 — Packets 4–5: task-local lifecycle and resolution
- Characterize all main/headless/service/subagent retry/fallback/terminal/reset boundaries.
- Implement task-owned load state, explicit `$` extraction before traffic, collision refusal, implicit policy, metadata budgeting, immutable child snapshots, and visible status.

### M4 — Packet 6: persistence compatibility
- Add old/new session/config goldens, no-rewrite reads, exact migration, drift warnings, portable omission, and bare/restrictive runner tests.
- Implement versioned refs while retaining old byte compatibility.

### M5 — Packets 7–8: read-only manager and exact enable store
- Add leading `ca skill list/show/enable/disable` with safe metadata inventory and no body/secret display by default.
- Add owner-only atomic exact profile records and prove list/show cause zero writes.

### M6 — Packet 9: install/import/new
- Reuse/generalize signed package primitives without weakening plugin collision rules.
- Add catalog signature/replay/traversal/decompression/source-drift tests and two-phase disabled publication.
- `new` uses the normal write approval path and leaves no partial tree on denial.

### M7 — Packets 10–11: TUI/legacy UX and pure pin resolver
- Preserve `/skills` aliases, add exact qualified management states, and review wide/narrow snapshots.
- Expose and test pure non-interactive `(instance_id, full_digest)` resolution with no ambient search.

### M8 — Packet 12: completion
- Run all focused and full gates, released disposable manager smokes, reconcile docs, mark complete, and archive.

## Acceptance Criteria

- Standard roots, deterministic duplicate instances, ancestor limits, and no additional-root bleed pass.
- Metadata/openai.yaml parsing, inventory/digest, safe-link policy, and all file/depth/byte bounds fail closed.
- Unique and ambiguous `$`, implicit-disabled policy, retry/fallback retention, terminal expiry, service isolation, and subagent immutable snapshots pass before provider traffic.
- `skill_view` cannot traverse, read drifted/oversize/nonregular files, execute scripts, or widen authority.
- Old sessions/configs remain readable without rewrite; new refs are exact and portable surfaces omit all skill bodies/resources/task loads.
- List/show are mutation-free; enable/disable are exact and atomic; install/import/new require preview/confirmation and publish disabled without overwrite.
- Bare, schedule, gateway, review, checkpoint, and other restrictive modes cannot obtain ambient skills.
- Focused suites, full tests, format, default/all-feature strict Clippy, release builds, help/version, and snapshot hygiene pass.

## Validation

Focused: baseline 11 skill tests pass. Record packet results as executed.
Completion: pending.
Manual/visual: pending disposable manager and wide/narrow TUI walkthroughs.
Blocked checks: live remote installs and real user imports are intentionally replaced by signed/local disposable fixtures.

## Progress

- [x] Binding harness, frozen handoff contract, existing skill plans/source, and primary portable specifications reviewed.
- [x] Active ExecPlan created with initial touched-file manifest.
- [x] P1 model and roots.
- [x] P2 parser and complete tree digest.
- [x] P3 read-only `skill_view`.
- [x] P4 task-local lifecycle.
- [x] P5 explicit/implicit resolution.
- [x] P6 persistence migration.
- [x] P7 read-only manager.
- [x] P8 enable/disable store.
- [x] P9 install/import/new.
- [x] P10 TUI/legacy UX.
- [x] P11 non-interactive pin resolver.
- [x] P12 completion gates/docs/archive.

## Decisions

- Decision: implement Agent Skills as an immutable instance layer alongside legacy APIs, then migrate callers packet-by-packet.
  Evidence: current callers and sessions depend on canonical legacy IDs while the frozen contract requires duplicate preservation.
  Consequence: compatibility remains testable throughout instead of requiring one unsafe flag-day rewrite.
- Decision: treat `allowed-tools` and declared dependencies as inspectable metadata only.
  Evidence: ca central selection/approval and the handoff explicitly forbid authority inheritance.
  Consequence: installation/loading cannot grant execution, network, path, MCP, or external action capability.
- Decision: keep historical oracle trees outside scope and use public specifications only as compatibility evidence.
  Evidence: AGENTS.md forbids copied oracle implementation and PARITY.md does not define Agent Skills as Cursor parity.
  Consequence: acceptance is ca behavior plus portable-format compatibility, not source or runtime equivalence.

## Surprises and Discoveries

- The initial Git baseline now makes packet diffs attributable, but the touched-file manifest remains mandatory.
- The OpenAI documentation budgets initial skill metadata by characters when context is unknown, while the frozen ca contract requires estimated tokens and 2%; ca will follow the stricter frozen contract and test deterministic estimation.

## Recovery and Rollback

Use packet-scoped `apply_patch` reversals against the root baseline. Do not reset, clean, or mutate oracle/reference trees. Persistent schema additions stay defaulted; manager tests use disposable homes and create-new destinations.

## Completion Record

Changed files: exactly the touched-file manifest above; no oracle/reference files, credentials, user configuration, or repository history were changed.
Commands and results: focused portable skills 17/17 PASS; focused service 20/20 PASS; full default 1,181 unit PASS plus 36 CLI PASS and one pre-existing manual browser test ignored; format, snapshot hygiene, default/all-feature strict Clippy, default release, and all-feature release bins PASS. Released help/version and disposable `skill list/new/install` preview/publication smokes PASS; published state was mode 0600 and disabled. Fixtures were moved recoverably to Trash.
Remaining risks: remote signed skill catalogs are intentionally not a separate wire format; local skill import uses exact content confirmation while signed curated distribution continues through the already-validated plugin catalog/package boundary. Subagents clone the current task prompt at runner start and do not share mutable load state.
Harness updates: queue marked complete, architecture/environment/testing/continuity/index reconciled, and this plan archived.
