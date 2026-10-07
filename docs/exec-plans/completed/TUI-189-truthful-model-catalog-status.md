# TUI-189 — Truthful Model Catalog Status

Status: COMPLETE
Owner/session: Codex desktop
Last updated: 2026-08-26

## Goal

The model picker must distinguish an optional live `/models` catalog failure
from provider unavailability. A provider with configured or documented models
remains a usable fallback; only a failed provider with no usable model source
is reported as unavailable.

## Non-Goals

- No provider protocol, authentication, routing, model-list, or session-format
  changes.
- No attempt to make the OpenAI Codex backend expose a public catalog endpoint.
- No runtime configuration or credential mutation.
- No oracle parity claim.

## Evidence and Current Behavior

- `src/main.rs::apply_model_catalog_refresh` labels every failed catalog lookup
  unavailable in the footer, even when the same provider is already counted as
  a documented fallback.
- The same function labels a failed OpenAI Codex catalog lookup unavailable
  despite retaining configured Codex models in the picker.
- `apply_provider_catalog_refresh` repeats the configured-model
  misclassification for a provider-scoped refresh.
- Read-only `ca --doctor --connect` on 2026-08-26 reported `openai-codex`
  working while the picker footer reported two unavailable providers.
- Inferred: a catalog refresh describes discovery freshness, not whether a
  configured route can execute requests.

## Scope

Allowed files: `src/main.rs`, focused TUI snapshots only if observable copy
changes, `NEXT_STEPS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`, `TESTING.md`,
`docs/.INDEX.md`, and this plan.

Forbidden files/areas: `oracles/`, `reference/`, provider wire protocols,
runtime config/session stores, credentials, and unrelated dirty files.

Runtime state affected: the validated installed binary may be replaced only
after completion gates, preserving the current binary recoverably and verifying
the installed artifact. No profile or provider state may change.

## Design and Invariants

- Live means a catalog refresh returned models for the provider.
- Fallback means live discovery did not succeed but configured or documented
  models remain usable.
- Unavailable means refresh failed and neither configured nor documented models
  can populate the picker.
- Provider rows retain sanitized failure detail so users can diagnose catalog
  freshness without being told the working route is unavailable.
- Footer counts are mutually exclusive and never count one provider as both a
  fallback and unavailable.
- Canonical model selection, provider calls, credentials, and persistence are
  unchanged.

## Milestones

### M1 — Characterization

- Add regressions for documented fallback, configured Codex fallback,
  provider-scoped configured fallback, and a truly model-less unavailable
  provider.
- Validation: focused tests fail against the existing classification.

### M2 — Classification repair

- Centralize usable fallback detection and apply it to full and provider-scoped
  refreshes.
- Build the footer from mutually exclusive live, fallback, and unavailable
  groups, naming genuinely unavailable providers.
- Validation: focused tests pass.

### M3 — Completion and delivery

- Run the serial workspace suite, format check, strict all-target/all-feature
  Clippy, all-feature release build, diff/snapshot/protected-tree hygiene, and
  release smoke checks.
- Update the harness, move this plan to completed, preserve the installed
  binary, install the verified release, and verify hash/signature/help/version.

## Acceptance Criteria

- A failed `openai-codex` catalog lookup with configured Codex models displays
  as `configured fallback`, not `unavailable`.
- A documented fallback is not also included in the unavailable count.
- A failed provider with no configured or documented models remains
  unavailable and is named.
- Provider-scoped refresh uses the same classification.
- No model, route, provider protocol, credential, or persisted state changes.

## Validation

Focused: initial configured-Codex and provider-scoped tests failed with
`unavailable`; the documented-fallback test failed with a double-counted
unavailable category. All focused catalog/fallback tests pass after the repair.

Completion: serial workspace tests, formatting, strict workspace
all-target/all-feature Clippy, workspace all-feature release build, release
smoke checks, diff/snapshot hygiene, and protected-tree checks pass.

Manual/visual: compare footer/provider copy with the supplied screenshot and
the confirmed working Codex doctor result.

Blocked checks: none currently.

## Progress

- [x] Traced full and provider-scoped refresh classification.
- [x] Added failing regressions.
- [x] Implemented mutually exclusive status classification.
- [x] Completed validation and harness reconciliation.
- [x] Installed and verified the validated binary.

## Decisions

- Decision: keep catalog failure detail while demoting it to fallback status
  whenever a usable configured/documented model source exists.
  Evidence: OpenAI Codex requests work independently of its failed catalog
  lookup.
  Consequence: users retain diagnostic information without a false provider
  availability claim.

## Surprises and Discoveries

- The footer currently counts a documented fallback a second time as
  unavailable because it derives the unavailable list directly from all
  failures.
- The picker deliberately supplies one `auto` row when no provider has any
  models, so the true-unavailable regression reports one visible model while
  still naming the provider as unavailable.
- The first serial workspace run found one older assertion that explicitly
  required a configured fallback provider to be labeled unavailable; the other
  2,058 CA tests passed. The assertion was updated to the new invariant before
  rerunning the full suite.

## Recovery and Rollback

Revert only the TUI-189 hunks with `apply_patch`. Do not reset the dirty
worktree. If installation occurs, restore the named TUI-189 backup by copying it
over the installed path after verifying both explicit paths.

## Completion Record

Changed files: `src/main.rs`, `NEXT_STEPS.md`, `CONTINUITY.md`,
`ARCHITECTURE.md`, `TESTING.md`, `docs/.INDEX.md`, and this completed plan.

Commands and results: focused tests pass; the final serial workspace run passes
2,061 CA tests, 38 CLI tests, 17 role-run tests, 15 watcher tests, three watcher
CLI tests, 556 scheduler tests, and all integration/doc targets, with three
documented manual/external checks ignored. Formatting, strict Clippy, release,
smoke, diff, snapshot, and protected-tree checks pass.

Remaining risks: a configured fallback can still have an unhealthy execution
route; the existing explicit provider doctor owns that separate health check.

Harness updates: `NEXT_STEPS.md`, `CONTINUITY.md`, `ARCHITECTURE.md`,
`TESTING.md`, and `docs/.INDEX.md` record the completed classification and
validation evidence.
