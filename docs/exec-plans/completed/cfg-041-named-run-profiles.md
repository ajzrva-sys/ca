# CFG-041 — Named Run Profiles

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-10

## Goal

Select one immutable whole-file run profile before runtime construction, with isolated config/credentials/sessions and an explicit non-persistent `bare` capability mode.

## Non-Goals

- Do not add runtime profile switching or an environment-variable selector.
- Do not migrate existing default config, credentials, or sessions.
- Do not let scripted `--test` runs inherit profile state.

## Evidence and Current Behavior

- Runtime paths currently expose one `config.toml` and one sessions directory; all mutation helpers target the single injected `ConfigStore`.
- Keychain accounts are provider-scoped only, so named configurations would otherwise collide.
- Ante documents whole-file named profiles and a bare mode; these are product lessons for a `ca` extension, not runtime or Cursor parity evidence.

## Scope

Allowed: CLI resolution, runtime/config/secret/session boundaries, capability gating, title/config/doctor output, tests/snapshots, and harness docs.
Forbidden: oracle/reference edits, real config/Keychain/session mutation, runtime switching, environment-based profile selection.

## Design and Invariants

- `ActiveProfile` validates lowercase ASCII names and resolves exactly once before doctor/list/headless/TUI construction.
- Default keeps the current paths and Keychain account format unchanged. Named profiles use `profiles/<name>.toml`, profile-scoped Keychain accounts, and profile-scoped session directories.
- Missing named profiles load defaults in memory and write no file until the first successful mutation.
- `bare` reads the normal config without creating it, then disables MCP, skills and activation, hooks, and session persistence. Persistent mutations are rejected with guidance while process-only model/reasoning changes remain available.
- All views and checks remain secret-free; no profile can delete another profile's credential.

## Milestones

- [x] Add validated profile/capability/path types plus CLI and path tests.
- [x] Make config creation lazy and namespace named-profile secrets/sessions.
- [x] Gate bare capabilities/mutations and expose active profile in title/config/doctor.
- [x] Add snapshots/smoke tests, run completion gates, reconcile docs, and archive this plan.

## Acceptance Criteria

- Positional prompt and `@file` parsing remain unchanged around `--profile`; `--test` rejects it.
- Missing named config does not write; its first successful mutation creates only that profile file and rollback remains atomic.
- Default/named credentials and sessions are isolated while default storage remains backward compatible.
- Bare provider/config/check views work, persistent operations clearly refuse, extensions and session persistence are absent, and retry/rewind/fork remain in memory.
- Non-default profile identity appears in the title, `/config`, and doctor text/JSON.

## Validation

Focused profile/config/secret/session/capability/CLI/render tests, then `cargo test --no-fail-fast`, format, strict Clippy, release build, and help/version/profile smoke tests.

Completed 2026-08-10: focused profile coverage passed; `cargo test --no-fail-fast` passed 431 unit and 25 CLI integration tests; formatting, strict Clippy, release build, help/version/profile smoke tests, and four reviewed wide/narrow profile snapshots passed. Smoke state used an empty temporary home and was removed afterward.

## Recovery and Rollback

Revert only CFG-041 files with targeted patches. Named test state lives only under temporary homes; default user files remain unmigrated.
