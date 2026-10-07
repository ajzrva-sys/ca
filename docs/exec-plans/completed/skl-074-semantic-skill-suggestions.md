# SKL-074 — Semantic Skill Suggestions

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Make relevant filesystem skills discoverable before submission and through `/skills <query>` by deterministic exact-ID, exact-alias, and lexical BM25 ranking, while never activating a suggestion or widening any existing prompt, tool, approval, workspace, profile, hash, session, or instruction-budget boundary.

## Non-Goals

- Automatic skill activation, high-confidence auto-load, implicit slash commands per skill, multi-skill orchestration, vector/embedding search, provider-assisted ranking, remote skill discovery/install, skill generation, or frontmatter expansion.
- Changing `activate_skill`, `--skill`, `/skills activate`, session restore, precedence, disabled identities, exact ID semantics, the 12,000-token active-body budget, or metadata catalog limits.
- Persisting queries, suggestions, ranks, scores, or dismissed state in config/sessions/history. Suggestions are derived process-only from already-discovered safe metadata.
- Injecting suggestions or ranking scores into provider messages. The existing bounded metadata catalog and explicitly active bodies remain the only skill prompt surfaces.
- Cursor parity. MiMo documents exact/alias/BM25 skill selection and automatic loading at `oracles/MiMo-Code-main/README.md:223`; SKL-074 deliberately adopts only local ranking and visible opt-in suggestions, not its automatic behavior.

## Evidence and Current Behavior

- `SkillRegistry` already validates bounded `name`, `description`, and up to 16 aliases, resolves project/user/compatibility precedence, excludes disabled/colliding entries, and stores metadata in deterministic ID order (`src/skills.rs`). Bodies are not read during search.
- `SkillRegistry::search` currently performs a case-insensitive substring filter in ID order. It provides no match reason, relevance order, score/coverage, result bound, or noise floor.
- `/skills [query]` renders that substring result into the generic pager; `/skills activate <id>`, `view`, `deactivate`, and `reload` are exact explicit actions (`src/main.rs`). The composer has no suggestion surface.
- `activate_exact` reloads and re-hashes the exact discovered path, refuses changed/disabled/unknown content, and enforces the shared active-body token budget. The model-facing `activate_skill` remains a central `Mutating` tool, so ordinary approval selection stays authoritative (`src/agent/{registry,tools}.rs`).
- Active session references persist only ID/path/SHA-256 and restore requires all three to match. Ranking state is unnecessary and must not enter Session V4 (`src/session.rs`).
- Baseline `cargo test skills --no-fail-fast` passed 10 unit/render tests and one CLI test on 2026-08-12.
- MiMo's README describes exact name, localized alias, and BM25 relevance plus automatic high-confidence loading. This is documentation evidence from a read-only oracle, not a clean runtime fixture or Cursor parity proof. Automatic loading conflicts with this roadmap's explicit boundary and is excluded.

## Scope

Allowed files: `src/skills.rs`; existing command/status rendering and focused tests/snapshots in `src/main.rs` and `src/tui/`; required harness documents and this plan. A tiny dedicated ranker module is allowed only if tests show `skills.rs` cannot remain cohesive.

Forbidden: oracle/reference edits or copied implementation; config/session schema changes; provider/network/filesystem activity during ranking; skill body reads; automatic activation; new authority/approval bypasses; real user skill/config/session/Keychain state; installed-binary changes.

Runtime state affected: none beyond the existing composer string, pager/status display, and skill registry. Suggestions are pure derived values over safe metadata and disappear when the query/composer changes or the process exits.

## Design and Invariants

### Ranking contract

- Add a safe borrowed or cloned `SkillSuggestion` view containing only metadata, match kind, and a deterministic display score/coverage. Match kinds are exact ID mention, exact alias mention, and related metadata. No path, SHA-256, body, diagnostics, or disabled/shadowed entry enters a suggestion.
- Normalize with Unicode lowercase and tokenize bounded alphanumeric runs. Treat `-` and `_` as both phrase-preserving identity characters and token boundaries for lexical matching. Empty/non-tokenizable queries return no suggestions; ranking performs no I/O.
- An exact full query or whole-identity mention of a skill ID ranks above an exact alias mention; both rank above lexical BM25. Alias matches always display the canonical activation ID and the matched alias.
- BM25 operates only over canonical ID, aliases, and description with field weights chosen and documented in `ca` tests rather than copied from an oracle. Apply a deterministic positive-score/noise floor and query-token coverage so one common token cannot flood the composer. Ties resolve by higher coverage, then canonical ID. Return at most three composer suggestions and a bounded command result set.
- Rebuilding/reloading the registry naturally rebuilds the corpus because ranking derives from current entries. Project/user precedence and disabled/collision filtering happen before ranking and remain unchanged.

### Visible, non-activating surfaces

- While idle with skills enabled, a non-slash composer draft may replace the generic status hint with at most three compact canonical IDs and match labels plus explicit `/skills <query>` guidance. No hint appears for empty input, command entry, busy turns, overlays, bare mode, no positive match, or a query whose only hits fail the noise threshold.
- `/skills` with no query retains the deterministic catalog. `/skills <query>` and `/skills search <query>` render ranked suggestions with canonical ID, active/origin state, aliases, match reason, and bounded sanitized description. Exact management verbs retain current semantics and never pass through ranking.
- The ranked pager says suggestions are not active and gives exact activation guidance. Suggestions never mutate `context_breakdown`, active refs, session state, input history, or provider requests.
- Narrow rendering truncates safely within the existing one-line status area. Wide/narrow snapshots cover exact ID, exact alias, lexical match, no match, and an already-active candidate.

### Compatibility and safety

- `SkillRegistry::search` may delegate to the new ranker for non-empty queries, but every caller must receive deterministic metadata-only results. Empty search preserves ID-order catalog behavior.
- Exact activation remains canonical-ID-only: an alias suggestion must not make `activate_exact(alias)` succeed. `/skills activate alias`, `--skill alias`, and `activate_skill {id: alias}` continue to fail.
- Ranking does not read skill files after discovery and does not compare bodies. A file changed after discovery may still be suggested from its safe cached metadata, but activation/view re-hashes and fails closed exactly as today.
- The existing metadata prompt is byte-for-byte unchanged. No automatic next-turn activation, implicit reminder, or extra tool call is introduced.

## Milestones

### M1 — Characterize and implement deterministic ranking

- Add failing temporary-root tests for exact ID/alias precedence, canonical alias result, BM25 relevance/ties, punctuation/Unicode, common-token/no-token noise, three-result bound, disabled/shadowed exclusion, metadata-only/no-I/O behavior, and changed-file activation refusal after suggestion.
- Implement the pure ranker and safe result types without adding dependencies or reading bodies.
- Validate with `cargo test skills --no-fail-fast`.

### M2 — Integrate ranked command and composer hints

- Add failing command tests proving `/skills` remains ID ordered while non-empty queries rank and explain suggestions, management verbs stay exact, and no activation/context/session mutation occurs.
- Add a render helper for idle composer hints and tests for lifecycle/overlay/bare/slash/no-match suppression plus UTF-8/narrow truncation.
- Add/review wide and narrow ranked-pager and composer-hint snapshots.

### M3 — Gates and released temporary-home walkthrough

- Run focused tests, full repository tests, format, strict Clippy, release build, and version/help/profile smokes.
- Under a disposable home/workspace with temporary project skills, run the rebuilt TUI at wide and narrow sizes: type a task that ranks a description match, inspect `/skills`, prove no skill body/request mutation or session write before explicit activation, explicitly activate the canonical ID, and prove the next loopback request receives only that exact hash-bound body. Change the skill after discovery and prove explicit activation fails closed if the walkthrough can do so without racing the TUI; otherwise rely on the hermetic unit boundary.
- Stop owned fixtures and move disposable state recoverably to Trash.

## Acceptance Criteria

- Exact canonical ID and alias mentions outrank lexical results deterministically; lexical results are bounded, positive, relevance-ordered, noise-filtered, and stable across discovery order.
- Suggestions expose only safe metadata and never read bodies, start provider/network/process work, activate a skill, or persist query/rank state.
- Composer hints are visible and actionable only when idle/appropriate, fit the existing status row, and disappear under busy/modal/bare/slash/no-match conditions.
- `/skills` empty catalog compatibility and exact management verbs remain intact. Alias suggestions name the canonical ID but aliases remain invalid activation IDs.
- Existing hash/path/disabled/budget/session/prompt/approval/tool/workspace boundaries pass unchanged; the metadata catalog prompt is byte-for-byte unchanged.
- All new TUI states have reviewed 100x30 and 60x24 snapshots, all repository gates pass, and released disposable behavior matches the tests without touching real state.

## Validation

Focused: baseline `cargo test skills --no-fail-fast` PASS (10 unit/render plus one CLI). Final `cargo test skill --no-fail-fast` PASS (24 unit/render plus two CLI integration tests), covering rank precedence/ties/bounds/noise, metadata-only behavior, hash-bound activation refusal, reload, exact management verbs, pager content, and composer suppression.

Completion: `cargo test --no-fail-fast` PASS (893 unit plus 28 CLI integration tests); `cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS; `cargo build --release` PASS. Release `--version`, `--help`, missing named-profile, and `bare` smokes passed without creating the missing named profile.

Manual/visual: eight reviewed 100x30/60x24 snapshots cover idle composer suggestions, exact-ID results, ranked alias/related results, and no-match guidance. A released-binary 100x30 disposable TUI walkthrough showed an idle canonical-ID hint before submission, a ranked `/skills` pager explicitly requiring activation, exact canonical activation, and one credentialless loopback request afterward. The request contained both safe catalog entries but only the explicitly active skill body; the inactive body was absent. Config hash remained unchanged, no session was written, the fixture was stopped, and the exact root was moved recoverably to `/Users/aji/.Trash/ca-skl074-walk.D3LXcD`.

Blocked checks: none currently.

## Progress

- [x] Existing registry/search/activation/prompt/session/command/render/oracle seams characterized.
- [x] Failing rank-contract tests and pure metadata ranker complete.
- [x] Ranked `/skills` results and idle composer hints complete.
- [x] Wide/narrow snapshots reviewed.
- [x] Repository-wide gates and released disposable walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: suggestions are user-visible process-only metadata, not model prompt content.
  Evidence: the roadmap explicitly forbids auto-activation, while the current prompt already exposes a bounded catalog and the mutating activation tool owns approval.
  Consequence: discovery improves before submission without creating a hidden provider/tool action or changing token accounting.
- Decision: aliases rank but never activate.
  Evidence: SKL-021 deliberately requires exact canonical IDs and persists canonical identity/path/hash.
  Consequence: every UI result must show the canonical activation ID even when an alias caused the match.
- Decision: implement bounded lexical BM25 locally without a new crate.
  Evidence: the corpus is small safe metadata already in memory, and deterministic field/token tests are more important than a dependency whose defaults may drift.
  Consequence: constants and token rules are `ca` product choices documented by tests, not oracle parity claims.

## Surprises and Discoveries

- `/skills` is currently a generated generic pager rather than a modal manager, so SKL-074 can add meaningful ranked discovery without a new input state machine.
- The model already receives a bounded metadata-only skill catalog and has a mutating exact-ID activation tool. Adding ranking to that prompt would duplicate context and tempt automatic behavior, so the initial surface stays entirely host-visible.
- Building the BM25 corpus on every ratatui status render would turn typing into repeated allocation. The registry now caches only tokenized safe metadata at discovery/reload; queries remain pure and body-free.
- Ratatui's diff renderer may emit a pager title in disjoint terminal spans even though the final screen is correct. The released walkthrough therefore used the stable pager page count for state transition and relied on reviewed TestBackend snapshots for exact rendered copy, while continuing to assert exact activation and provider-request isolation end to end.

## Recovery and Rollback

Remove the pure ranking/result types and composer-status branch, then restore non-empty `/skills` filtering to the current substring predicate. No persisted schema or runtime file requires migration. Exact activation/session references remain unchanged throughout.

## Completion Record

Changed files: `src/skills.rs`; `src/main.rs`; `src/tui/render.rs`; `src/tui/render_tests.rs`; eight `skill_suggestions_*` snapshots; this ExecPlan; and the required harness documents.

Commands and results: baseline and final focused skill suites, the full 893-unit/28-CLI test run, formatting, strict Clippy, release build, CLI/profile smokes, snapshot review, and the released disposable TUI/loopback walkthrough all passed as recorded above.

Remaining risks: lexical relevance is intentionally bounded and deterministic rather than semantic-vector search, so synonyms absent from safe metadata may not surface. Suggestions never compensate by loading bodies or contacting a provider; users retain exact `/skills` catalog/activation control.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the ranker, non-activation invariant, validation, extension status, and `CHG-075` handoff.
