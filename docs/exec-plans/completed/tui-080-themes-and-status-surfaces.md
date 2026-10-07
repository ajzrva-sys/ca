# TUI-080 — Themes and Status Surfaces

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Add idle-only `/theme` and `/statusline` managers that persist validated profile-scoped display choices, preserve the existing appearance and status text by default, and optionally render one bounded process-only custom status command whose execution is sandboxed with network denied.

## Non-Goals

- Arbitrary color literals, downloadable themes, terminal-color detection, GUI preferences, model-visible status output, shell prompt integration, background network polling, per-workspace themes, runtime profile switching, accessibility/VoiceOver work, or Cursor parity.
- Weakening the saved/effective sandbox mode, running a custom command without an enforceable network-denied sandbox, or storing command output in config, sessions, requests, usage, or Debug state.

## Evidence and Current Behavior

- `src/tui/render.rs::render_title` hard-codes a cyan badge and dark-gray hint bar; `render_status` hard-codes message/token/help text and semantic colors.
- `src/config.rs::CaConfig` has no UI settings. `ConfigStore` already provides comment-preserving atomic patches, profile-specific paths, lazy named-profile creation, and bare-mode write rejection.
- `AppState` already carries immutable `ActiveProfile`, `RunCapabilities`, current model/mode/profile/workspace values, and process-only status state; it has no theme/statusline manager.
- `src/sandbox.rs::SandboxBackend` can construct a required macOS sandbox profile with network denied. Policy-only/off modes do not enforce network denial and therefore cannot safely run custom status commands.
- Inferred: built-in theme application must begin at shared principal chrome and semantic status surfaces rather than mechanically rewriting every legacy `Color::Cyan`, so the default remains visually and snapshot compatible while new choices are coherent.

## Scope

Allowed files: a new bounded UI-settings module; narrow additions in `src/{config,app,main}.rs`; `src/tui/{commands,input,render,render_tests}.rs`; focused snapshots; this plan and required harness documents.

Forbidden files/areas: oracle/reference trees; real user config/sessions/Keychain; installed binary; provider/request/tool/session schemas; accessibility; arbitrary plugin/theme loading.

Runtime state affected: selected profile config only after explicit save; process-only manager state, active palette, custom command generation/output/error. Tests and walkthroughs use disposable config roots and fake commands only.

## Design and Invariants

### Validated persisted settings

- Add a defaulted `[ui]` config with a closed built-in theme enum, an ordered duplicate-free status-field list, an optional custom status command, and a positive bounded refresh interval. Missing UI config reproduces the current title/status appearance and text.
- Config loads validate every value before runtime activation. Mutations replace only exact UI keys while preserving unrelated TOML and comments. Named profiles remain isolated/lazy; bare mode loads the normal config's UI settings but all manager saves are disabled with existing guidance.

### Managers and rendering

- `/theme [filter]` opens a searchable built-in list, marks current, previews the highlighted palette without persistence, Enter saves/applies, and Esc restores the saved palette. No free-form colors enter state.
- `/statusline` opens an ordered built-in field manager. Space toggles, Shift+Up/Down reorders enabled fields, Enter saves, and Esc discards the draft. Essential dynamic/busy/error/paused notices continue to override the idle statusline.
- The built-in default fields reproduce the current `messages`, optional `tokens`, send/history/commands hints. New fields may expose only already-safe values such as model, profile, mode, workspace basename, and Git branch.

### Custom command boundary

- A configured custom command is disabled by default and never editable in the TUI; `/statusline` exposes its configured/enabled state and an explicit test action.
- Execution requires the required sandbox backend, uses workspace read authority with a private temporary directory, always denies network, has a one-second deadline, kills its process group on timeout/cancel, and reads at most 4 KiB combined safe UTF-8 output. Nonzero, unavailable, timed-out, and oversized results are typed/safe and never include stderr or command text.
- At most one generation runs at a time. A newer test/refresh or manager close invalidates stale completion. Normal refresh is bounded and never blocks drawing/input.

## Milestones

### M1 — Characterization and validated config

- Add failing tests for command discovery, default visual/status compatibility, config validation, comment preservation, named-profile isolation, lazy creation, and bare rejection.
- Implement typed theme/status settings and exact config patches.

### M2 — Managers and custom runner

- Add manager state/input/actions/rendering plus wide/narrow snapshots for theme list/preview, status fields, custom configured/unavailable/testing/failure, and bare read-only states.
- Implement the bounded fail-closed custom command runner and stale-generation orchestration.

### M3 — Gates and disposable walkthrough

- Run focused/full gates and release smokes, then use a temporary home and fake command to prove save/restart, profile isolation, status ordering, sandbox enforcement, output bounds, and zero writes from bare mode.

## Acceptance Criteria

- With no `[ui]` table, existing title and idle status snapshots remain unchanged. Every built-in theme changes the principal badge, selection/accent, and semantic status palette coherently.
- Invalid theme/field/interval config fails before activation. Saves preserve unrelated comments and affect only the selected profile; bare previews but cannot write.
- `/theme` preview is reversible; `/statusline` toggling/reordering is deterministic and persists only on Enter.
- A custom command cannot run without an enforceable network-denied sandbox, cannot overlap, cannot outlive timeout/cancellation, and cannot emit more than 4 KiB or enter persistent/model-visible state.
- Focused and repository-wide validation pass without oracle or real runtime-state mutation.

## Validation

Focused: PASS. Theme, statusline, UI-settings runner, config-patch, orchestration, command/input, and unchanged-default render characterization all passed. Twenty-two reviewed snapshots cover wide/narrow preview, no-match, configured/unconfigured, testing, busy, failure, and bare states.

Completion: PASS. `cargo test --no-fail-fast` passed 977 unit tests plus 28 CLI integration tests; `cargo fmt --check`, strict Clippy, release build, help/version, and isolated named/bare profile smokes passed.

Manual/visual: PASS. The released binary, disposable home, required macOS sandbox, and harmless `printf status-ok` command proved theme preview/save/restart, status ordering/reordering/save/restart, explicit status testing, periodic refresh, hidden command text, and bare preview-only refusal with an unchanged config checksum. The inspected fixture moved recoverably to `/Users/aji/.Trash/ca-tui080-smoke.0jC8vo`.

Blocked checks: none known.

## Progress

- [x] Existing title/status/config/profile/sandbox seams traced.
- [x] Failing command/config/render/runner characterization added.
- [x] Validated persisted settings and managers implemented.
- [x] Focused/full gates and disposable released walkthrough pass.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: persist a closed typed theme identifier, not arbitrary colors.
  Evidence: unrestricted terminal styles create unreadable combinations and make snapshot/security validation unbounded.
  Consequence: themes are deterministic, reviewable, and backward compatible.
- Decision: custom command text remains config-file-only.
  Evidence: a shell editor inside a status manager expands both secret and approval surfaces without helping normal field customization.
  Consequence: the manager may enable/test a configured command but never snapshots or echoes it.
- Decision: fail closed unless network denial is enforceable.
  Evidence: policy-only/off sandbox modes explicitly do not constrain processes.
  Consequence: optional status commands are unavailable rather than silently less safe.

## Surprises and Discoveries

- The renderer contains many local cyan styles across historical overlays. Applying a palette mechanically would create a risky whole-file rewrite; this plan centralizes principal/semantic theme surfaces and migrates shared accent helpers only where covered by snapshots.
- A one-second configured refresh makes the manager's safe result label visibly alternate with its short `testing...` state. The production default remains five seconds, and the single-generation lock prevents overlap.

## Recovery and Rollback

Remove the UI config/module/state/command/render additions with local patches. The default config path and missing `[ui]` behavior are unchanged; persisted `[ui]` keys are defaulted and ignored by older builds, so no migration is required.

## Completion Record

Changed files: `src/ui_settings.rs`; narrow changes in `src/{config,app,main}.rs` and `src/tui/{commands,input,render,render_tests}.rs`; 22 focused snapshots; this plan and the required harness documents.

Commands and results: focused theme/statusline/UI/config/orchestration suites PASS; `cargo test --no-fail-fast` PASS (977 unit + 28 CLI); `cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS; `cargo build --release` PASS; released help/version/profile smokes PASS; disposable released-TUI walkthrough PASS.

Remaining risks: principal chrome and semantic states use the selected palette, while historical one-off overlay colors remain intentionally unchanged until a separately tested migration. Custom status commands remain optional, fail closed without enforceable network denial, and can still consume local CPU for at most their one-second deadline.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the completed extension and evidence.
