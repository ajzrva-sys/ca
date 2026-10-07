# SES-051 — Saved Session Retention

Status: COMPLETE
Owner/session: `/root`
Last updated: 2026-08-11

## Goal

Let users configure optional age/count retention rules, inspect the exact inactive sessions those rules would delete, and permanently delete only an explicitly confirmed preview while always protecting the current, pinned, and archived sessions.

## Non-Goals

- Automatic startup/background pruning, deletion without a preview, scheduled cleanup, archive retention, branch cascades, disk quotas, or session-content inspection.
- Changing Session V4, rewriting retained sessions, deleting imported/parent/child sessions as a group, or refreshing recency on resume.
- CLI pruning flags, runtime profile switching, retention environment variables, or retention in `bare`.
- Cursor parity. Hax and Hermes documentation demonstrate age-based retention and dry-run/prune concepts, but this is an oracle-informed `ca` extension without a clean Cursor fixture or runtime observation.

## Evidence and Current Behavior

- `src/config.rs::CaConfig` has no session-retention settings. `ConfigStore` already provides comment-preserving atomic scalar patches and disables writes for the `bare` profile.
- `src/session.rs::SessionSummary` exposes storage key, UUID, pin state, and `updated_at`; `SessionStore::list` sorts pinned-first/newest and `SessionStore::delete` re-reads and UUID-validates one exact JSON file.
- Active and archived sessions have separate profile-scoped roots. `/resume` already protects the current session and never cascades exact-file delete/archive actions to descendants.
- Session mutations run on blocking workers, with typed busy/error states and stale-generation rejection in `src/main.rs`, `src/app.rs`, and `src/tui/input.rs`.
- `oracles/hax-master/docs/configuration.md` documents configurable age retention and `0` as disabled, while Hermes documents an explicit dry-run/prune surface. The approved `ca` behavior is deliberately safer: absent settings are disabled and no deletion occurs until the exact TUI preview is confirmed.

## Scope

Allowed files:

- New `src/session_retention.rs` plus `src/{config,session,app,main}.rs`
- `src/tui/{commands,input,render,render_tests}.rs`, focused tests, and deterministic snapshots
- `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`
- This ExecPlan

Forbidden files/areas:

- `oracles/`, `reference/`, real user configuration, real saved sessions, Keychain entries, external services, and the installed binary
- Session import/export schema, archive deletion, Session V5/lazy history, hooks, scheduler, or automatic pruning

Runtime state affected:

- An ordinary profile config only after explicit retention-editor save.
- Exact active session JSON files only after a Cancel-first preview confirmation. Tests and walkthroughs use exact disposable roots.

## Design and Invariants

- Add defaulted `[sessions] retention_days` and `retention_count` optional positive integers. Missing fields disable their respective rules; both missing means retention is fully disabled. Reject zero, overflow, age above 36,500 days, and count above 100,000 before writing.
- Add visible idle-only `/retention` plus a Session retention row in `/config`. Opening either performs no mutation and owns a dedicated process-only manager with Overview, Edit, Preview, Saving, and Deleting states.
- Overview shows enabled/disabled rules, active/eligible/protected counts, and that archived sessions are excluded. `Shift+E` edits safe scalar fields; blank disables a rule. `Enter` builds a fresh preview. `Esc` closes.
- Saving settings is comment-preserving and atomic, never performs deletion, and recomputes state only after config reload. Bare is view-only. A `--no-session` run may edit ordinary-profile settings for future runs but cannot preview/delete because it has no persistent session store.
- Preview uses one captured `now` and the freshly listed active summaries. Current UUID and every pinned summary are excluded before either rule is evaluated; archived summaries are never inputs. Age matches only `updated_at < now - retention_days`. Count keeps the newest `retention_count` eligible summaries and selects the remainder. The deletion set is the deterministic union, sorted oldest then UUID, with typed Age, Count, or Both reasons.
- A preview stores every exact storage key/UUID/title/timestamp/reason to be deleted and the settings/generation that produced it. At most 10,000 deletions may enter one preview; larger sets fail closed with guidance rather than showing a partial preview.
- Confirmation is Cancel-first. Empty previews cannot be confirmed. Editing settings, list refresh, cancellation, or a newer preview invalidates the prior generation.
- Confirmed deletion returns the exact preview plus current UUID to `SessionStore`. Before unlinking anything, the store validates every filename component, re-reads every UUID, and verifies that none is current or newly pinned. Only after the whole preflight succeeds are exact active-root JSON files unlinked; archived roots and descendant metadata are untouched.
- An unlink failure may leave a truthful partial result because multi-file filesystem deletion cannot be atomic. The manager refreshes both lists, keeps the preview/error visible, reports the exact removed/remaining counts, and never claims rollback. Preflight failures delete nothing.
- All config and filesystem work runs on one blocking mutation worker. Busy state locks actions and paste cannot reach hidden composer/editor fields. Stale worker results are discarded.

## Milestones

### M1 — Characterize config and deterministic planning

- Add failing tests for absent/default settings, strict positive/bounded validation, comment-preserving patches, bare write refusal, age boundary, count ordering, rule union, current/pinned exclusion, and 10,000-entry preview bound.
- Add store tests for whole-batch preflight, UUID/path/pin/current rejection, exact deletion, archive/descendant preservation, and no-write failure.
- Validation: focused config/session tests fail only on missing SES-051 behavior.

### M2 — Implement retention core

- Add the defaulted config types/patch, deterministic typed preview builder, and exact preflighted batch deletion boundary.
- Keep source timestamps and session files unchanged until explicit apply.
- Validation: core retention suites pass in temporary roots with an injected clock.

### M3 — Add command and asynchronous TUI manager

- Add `/retention`, `/config` routing, overview/editor/preview/confirmation state, blocking save/delete orchestration, busy/error/stale handling, and predictable refresh.
- Add reviewed wide/narrow snapshots for disabled, configured/no-candidates, populated preview, Cancel-first confirmation, busy deletion, bare view-only, and inline failure.
- Validation: command/input/orchestration/render suites and snapshot review pass.

### M4 — Complete gates and isolated walkthrough

- Run focused suites, `cargo test --no-fail-fast`, formatting, strict Clippy, release build, and help/version/profile smoke tests.
- Under a temporary home, seed current, pinned, archived, recent, age-expired, and count-overflow sessions. Save rules, cancel one preview, confirm the next, and inspect exact survivors/files/profile isolation. Move or trash only the recorded temporary root.
- Reconcile the five harness documents, move this plan to completed, and mark HOK-052 next without starting it.

## Acceptance Criteria

- Retention is disabled for all existing configs and never runs automatically.
- Settings save atomically without changing unrelated TOML/comments; blank fields disable rules and invalid values remain in the editor with no write.
- Every preview is complete and deterministic for one settings/list/time snapshot. Current, pinned, and archived sessions never appear.
- Cancel is selected by default and causes no filesystem mutation. Empty or oversized previews cannot be confirmed.
- Apply revalidates the full exact batch before deletion; traversal, UUID mismatch, current-session membership, or newly pinned state causes zero deletions.
- Successful apply deletes only the previewed active files and never rewrites/cascades descendants. Partial unlink failures are reported honestly and refreshed.
- Bare never writes settings or sessions; no-session cannot preview/delete persistent sessions.
- Wide/narrow snapshots cover overview, preview, confirmation, busy, failure, and view-only states; all focused/full gates and the isolated walkthrough pass.

## Validation

Focused:

- `cargo test session_retention --no-fail-fast`: PASS, 16 config/core/command/input/orchestration/render tests.
- `cargo test retention_batch --no-fail-fast`: PASS, 3 exact-delete/preflight/partial-failure store tests.
- `cargo test command_palette_snapshots --no-fail-fast` and `cargo test config_hub --no-fail-fast`: PASS; the new command/config route is discoverable and zero snapshots remain pending.

Completion:

- `cargo test --no-fail-fast`: PASS, 576 unit tests plus 25 CLI integration tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS after boxing the large config-bearing async result variant.
- `cargo build --release`: PASS.
- Rebuilt help/version/profile smoke: PASS; the missing named profile remained unwritten and bare resume failed with exit 2.

Manual/visual:

- Reviewed 14 wide/narrow retention snapshots covering disabled, edit, empty/populated preview, deletion busy, bare, and failure states, plus the intentional wide/narrow command-palette addition.
- Isolated rebuilt-binary TUI walkthrough: PASS. Saving 30-day/keep-2 rules preserved all seven active sessions and the config comment; the exact preview listed three candidates while excluding current, pinned, and archived sessions; default Cancel preserved every file; explicit confirmation removed only the three previewed files.

Blocked checks:

- None currently.

## Progress

- [x] Traced config patching, session-summary/store, profile/archive, and `/resume` mutation boundaries.
- [x] Reviewed retention/dry-run documentation as product evidence only.
- [x] Created the active ExecPlan before implementation.
- [x] Added failing config/session/command/input/orchestration/render characterization tests.
- [x] Implemented retention config, deterministic preview, and exact batch deletion.
- [x] Implemented the asynchronous TUI manager and reviewed snapshots.
- [x] Passed focused and repository-wide validation gates.
- [x] Completed the isolated walkthrough and reconciled documentation.

## Decisions

- Decision: retention is explicit preview-and-confirm, never an automatic startup/background sweep.
  Evidence: the approved requirement says every deletion receives a preview with Cancel selected; automatic pruning cannot satisfy that interaction.
  Consequence: configured rules remain useful and persistent without introducing unattended destructive behavior.
- Decision: count applies only to otherwise deletable active sessions and keeps the newest eligible rows.
  Evidence: current and pinned sessions are mandatory exclusions, not quota consumers that should force additional deletion.
  Consequence: protection never causes surprising deletion of a newer ordinary session.
- Decision: batch apply preflights all candidates but reports honest partial failure after unlink begins.
  Evidence: multiple independent filesystem unlinks cannot be made atomically without adding a separate trash/transaction format outside this slice.
  Consequence: validation/security races fail before mutation; rare device errors after preflight remain visible and recoverable only through backups, not a false rollback claim.

## Surprises and Discoveries

- Hax documents automatic age pruning enabled by default, which conflicts with the approved disabled-by-default/always-preview boundary. It is evidence for setting vocabulary only, not adopted behavior.
- `SessionSummary.updated_at` is already sufficient for deterministic age/count planning; no Session V4 or filesystem-mtime migration is required.
- Strict Clippy exposed that returning a full `CaConfig` inline made the retention/stream message enums unnecessarily large. Boxing only that async payload preserves ownership and keeps the event enum bounded.

## Recovery and Rollback

- Code rollback is patch-only and limited to SES-051-owned files.
- Cancelling or failing before batch apply changes no session files. Config saves use the existing atomic writer.
- Confirmed retention deletion is permanent by explicit design. Tests/walkthroughs use disposable roots; no Git reset or real-session cleanup is allowed.

## Completion Record

Changed files: `src/session_retention.rs`, `src/{config,session,app,main}.rs`, `src/tui/{commands,input,render,render_tests}.rs`, reviewed TUI snapshots, the five harness documents, and this ExecPlan.

Commands and results: the focused suites, 576-unit/25-CLI full test run, formatting, strict Clippy, release build, help/version/profile smoke, and isolated TUI walkthrough all passed as recorded above.

Remaining risks: confirmed multi-file unlink can still fail after one or more successful deletions; the manager reports exact removed/remaining counts and refreshes instead of claiming rollback. Retention remains deliberately manual and TUI-only.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, and `PARITY.md` record the explicit-preview retention boundary and keep it outside Cursor parity claims. Disposable roots were moved to `/Users/aji/.Trash/ca-ses051-walk.TgWWt1` and `/Users/aji/.Trash/ca-ses051-smoke.cpmQaf`.
