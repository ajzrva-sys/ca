# AUTH-104 — Automated Provider Authentication

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-14

## Goal

Make `/provider` a connection-first picker for Xiaomi MiMo, OpenAI Codex, DeepSeek, and direct OpenAI on macOS and FreeBSD: authenticate using each provider's supported contract, store credentials safely, probe once, configure the starter model, and switch only after a successful probe.

## Non-Goals

- Inventing OAuth for DeepSeek or direct OpenAI API keys.
- Browser-cookie scraping, passwords, or copying implementation from repository oracles.
- Persisting provider configuration in bare mode.
- Printing, logging, snapshotting, or live-test capturing credentials or provider response bodies.

## Evidence and Current Behavior

- `src/tui/input.rs::open_provider_manager` and `handle_provider_list` expose configured providers only; Enter always opens models.
- `src/provider_secrets.rs` stores direct keys only in macOS Keychain and emits `keychain://` references; non-macOS storage is unavailable.
- `src/codex_auth.rs` has browser PKCE, refresh/revoke, and strict reads of `~/.codex/auth.json`, but cannot create a missing file cache on non-macOS.
- `src/main.rs::resolve_codex_oauth_provider` injects the Codex token in only selected normal request paths. `src/doctor.rs::add_connection_checks` probes the unresolved config and can therefore report a false 401.
- Official OpenAI authentication documentation permits browser sign-in, prefers device-code authentication for headless Codex, and documents file/keyring credential caching shared by Codex clients.
- Xiaomi's published MiMoCode client contract uses an ephemeral X25519 public key in `/authorize`, then decrypts the returned base64url X25519/AES-256-GCM envelope locally.
- DeepSeek publishes bearer API-key authentication only.

## Scope

Allowed files: provider authentication/storage/config/runtime modules; `/provider` state, input, render, snapshots; provider probe/doctor/request integration; Cargo metadata; focused tests and repository harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, unrelated feature implementations, user runtime configuration, user credentials, installed binaries.

Runtime state affected: only after explicit interactive authentication, macOS Keychain or owner-only profile data credentials may change; Codex OAuth may update the account-global owner-only Codex cache. Automated tests use disposable paths and fixtures only.

## Design and Invariants

- `credential://ca/...` is the canonical new config reference. Existing exact `keychain://ca/...` and `${ENV_VAR}` values remain readable.
- The system secret backend is Keychain on macOS and a profile-data credential directory on other Unix systems. Portable files require current-user ownership, regular-file/no-follow reads, bounded data, directories `0700`, files `0600`, and atomic replace.
- Authentication produces a candidate provider configuration. The candidate is probed before selection. A 401/403 restores the prior credential/config; a non-auth probe failure retains the candidate but does not change the selected model.
- OpenAI Codex authentication stays separate from direct OpenAI API-key authentication and uses the account-global Codex cache.
- MiMo uses only the returned account-specific base URL and secret key; manual/headless input is masked.
- TUI work remains asynchronous and cancellation-safe. Errors contain safe classifications, never secrets or provider response bodies.
- Every provider request path resolves credentials through one shared boundary before client construction, including doctor, probe, catalog refresh, headless, and TUI requests.

## Milestones

### M1 — Portable credential resolver and persistence

- Add canonical credential references and legacy read compatibility.
- Implement hardened profile-scoped file storage and bare-mode memory behavior.
- Add mode, ownership, symlink, bounds, atomicity, isolation, deletion, rollback, and redaction tests.
- Validation: focused provider-secret/config tests.

### M2 — Provider authentication primitives

- Add MiMo X25519 authorization URL/envelope decryption and loopback/manual flows.
- Add OpenAI device-code flow and safe cache creation/preservation.
- Add direct masked key candidates for DeepSeek/OpenAI.
- Validation: deterministic HTTP/crypto/cache fixtures, timeout and cancellation tests.

### M3 — Unified `/provider` workflow

- Add four first-class rows, connection states, Enter/Shift+L behavior, auth modal/progress/manual input, candidate persistence/probe/rollback/selection.
- Preserve advanced template/editor flows.
- Validation: input state tests and wide/narrow render snapshots.

### M4 — Runtime unification and completion

- Route doctor, probes, model refresh, headless, TUI, and review/auxiliary requests through the shared resolver.
- Update architecture/environment/testing/continuity/queue/index documentation.
- Run full macOS and FreeBSD gates and release builds. Run live marker requests only when credentials are already available without reading or printing them.

## Acceptance Criteria

- `/provider` always shows Xiaomi MiMo, OpenAI Codex (ChatGPT), DeepSeek, and OpenAI API with a disconnected/connected status.
- Enter authenticates a disconnected first-class provider; Enter opens models for a connected provider; Shift+L replaces credentials.
- API-key/manual-result input is masked; browser/device URLs and one-time non-secret codes remain fully selectable.
- Successful auth plus probe persists canonical config and selects the documented starter model.
- 401/403 restores prior authentication; other probe failures retain the new credential/config without switching.
- FreeBSD secrets and created Codex cache meet ownership/mode/no-follow/bounds/atomicity requirements.
- Existing environment references and legacy Keychain references continue to resolve.
- Doctor and normal provider requests resolve OpenAI Codex identically.
- Focused tests, all tests, format, strict all-target Clippy, and release builds pass on macOS and physical FreeBSD, or unavailable live/account checks are recorded as blocked without guessing.

## Validation

Focused: provider-secret/config, MiMo crypto/callback, Codex browser/device/cache,
candidate probe transaction, TUI input, and wide/narrow snapshot tests pass.

Completion: macOS `cargo test --all-targets` passed 1,265 unit tests with one
ignored manual browser test and 36 CLI tests; format, strict all-target Clippy,
and release build passed. Physical FreeBSD 15.1 amd64 passed locked strict
all-target Clippy, 1,252 owner-matching unit tests with one ignored manual
browser test, 36 CLI tests, release build, and binary smokes.

Manual/visual: reviewed first-class picker and browser/device/manual/key modal
snapshots at wide and narrow sizes. Existing macOS Codex authentication passed
both `--doctor --connect` and a normal disposable-config marker request returning
`CA-AUTH104-OK` with `gpt-5.6-sol`. The physical FreeBSD `aji` account's
owner-only cache likewise produced a working Codex doctor row and a normal
request returning exactly `CA-AUTH104-FREEBSD-OK`.

Blocked checks: live MiMo, DeepSeek, and direct OpenAI requests were unavailable
because those credentials or interactive sessions were absent. They were not
synthesized, printed, or copied.

## Progress

- [x] Captured the dirty-worktree baseline and re-verified the official OpenAI browser/device/cache contract.
- [x] Implement portable credential storage and resolver.
- [x] Implement MiMo and OpenAI OAuth/device primitives.
- [x] Implement the unified provider picker/authentication state machine.
- [x] Add focused protocol, storage, input, and snapshot tests.
- [x] Reconcile documentation and run completion gates on macOS and FreeBSD.

## Decisions

- Decision: DeepSeek and direct OpenAI use masked API-key input.
  Evidence: both publish bearer-key authentication; DeepSeek publishes no OAuth/device flow.
  Consequence: no fake browser login is presented for those rows.
- Decision: Codex subscription auth and direct OpenAI API auth remain separate rows and credentials.
  Evidence: official OpenAI docs distinguish ChatGPT sign-in from platform API-key billing/authentication.
  Consequence: replacing one route never overwrites the other.
- Decision: preserve all pre-existing dirty-tree changes and add only scoped patches.
  Evidence: baseline contains 44 modified files and multiple untracked completed plans/features.
  Consequence: validation and final reporting must describe only AUTH-104 edits.

## Surprises and Discoveries

- The existing OpenAI browser flow stores tokens before any provider probe, so transaction/rollback behavior must be introduced explicitly rather than layered only in the UI.
- Native owner/mode tests must run under an owner-matching unprivileged account; root legitimately bypasses Unix permission checks and is not evidence for the portable-file boundary.

## Recovery and Rollback

Each persistence mutation keeps the prior config/credential until its replacement is validated. Test artifacts use temporary directories. Source rollback is by reversing the scoped patch only; never reset the dirty worktree.

## Completion Record

Changed files: provider secret/config/Codex/MiMo/connect/probe/catalog modules;
main/App/TUI input, commands, render and snapshots; repository harness docs.

Commands and results: focused Cargo tests passed; macOS full tests, format,
strict all-target Clippy, and release passed; physical FreeBSD locked strict
all-target Clippy, unprivileged full unit tests, CLI tests, release, help/version,
ELF and linked-library smokes passed.

Remaining risks: provider availability and undocumented future upstream auth
contract changes remain external. Live MiMo, DeepSeek, and direct OpenAI flows
require the user to complete `/provider` authentication.

Harness updates: protocol/storage fixtures, rollback tests, input tests, and
wide/narrow snapshots added; validation results recorded in `TESTING.md`.
