# BSD-103 — FreeBSD Regression Audit

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-14

## Goal

The exact current working tree builds, tests, and produces a usable release binary on the physical FreeBSD 15.1 amd64 `bsd` host without changing installed binaries, credentials, configuration, sessions, or project workspaces.

## Non-Goals

- Adding a FreeBSD mandatory sandbox backend.
- Enabling macOS Keychain, CoreAudio, or LaunchAgent features on FreeBSD.
- Installing `ca` or exercising paid/provider traffic.
- Changing oracle or reference trees.

## Evidence and Current Behavior

- `cargo check --locked` passes natively on FreeBSD 15.1 with Rust/Cargo 1.96.1.
- The 21 workspace-materialization tests and the platform-name test pass as `nobody`.
- The complete native unit binary currently reports 1,216 passed, 17 failed, and 1 ignored.
- Failures identify portable target naming (`x86_64` rejected by a hyphen-only manifest contract), hard-coded macOS/Linux executable paths, macOS-only required-sandbox tests running on FreeBSD, one helper using required mode despite the documented FreeBSD policy-only boundary, and an overly short loopback first-event deadline.

## Scope

Allowed files: `src/{agent/client_tests.rs,agent/tools.rs,external_runtime.rs,self_update.rs,schedule_v2.rs,lsp.rs,local_models.rs,ui_settings.rs,voice.rs,main.rs,service.rs}`, `tests/cli.rs`, this plan, `NEXT_STEPS.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, and `TESTING.md`.

Forbidden files/areas: `.git`, `oracles/`, `reference/`, user runtime state, installed binaries, credentials, and unrelated existing edits.

Runtime state affected: disposable `/tmp/ca-freebsd.audit.*` source/build trees and a disposable `/tmp/ca-freebsd.audit-home` on `bsd` only.

## Design and Invariants

- Preserve the fail-closed production `required` sandbox on non-macOS platforms.
- Normalize runtime/update target architecture names to the existing lowercase-hyphen manifest grammar.
- Resolve packaged Git from a closed absolute candidate list and retain a cleared environment.
- Make test fixtures discover the host's existing Python without changing production LSP/runtime configuration.
- Platform-gate tests whose asserted capability is explicitly macOS-only.

## Milestones

### M1 — Characterize

- Reproduce native locked compilation and focused/full tests in a disposable copy.
- Classify failures by production portability versus test-only platform assumptions.

### M2 — Repair

- Add focused target-name coverage and portable Git/Python fixture selection.
- Keep required-sandbox behavior unchanged while making tests reflect documented platform support.
- Relax only the cancellation test's loopback deadline.

### M3 — Validate

- Run local format, focused tests, full tests, strict Clippy, and release build.
- Repeat locked native FreeBSD tests, Clippy, release build, and help/version smokes.

## Acceptance Criteria

- Current FreeBSD target names satisfy the signed runtime/update grammar.
- Scheduled Git materialization finds packaged FreeBSD Git without inheriting the ambient environment.
- Native unit and CLI tests pass, excluding only explicitly ignored tests.
- Native strict Clippy and release build pass without BSD-specific warnings.
- Released FreeBSD binary reports help/version and is an amd64 FreeBSD ELF.

## Validation

Focused: PASS — target grammar, scheduled Git materialization, background jobs,
receiver cancellation, LSP/local-model fixtures, workspace materialization, and
headless MCP tests passed locally and/or natively as applicable.

Completion: PASS — local 1,243 unit tests plus 36 CLI tests; FreeBSD 1,230
applicable unit tests plus 36 CLI tests; both hosts passed formatting, strict
all-target Clippy, and release builds.

Manual/visual: PASS — released FreeBSD help/version smokes, ELF inspection,
dynamic-library inspection, SHA-256, and unchanged disposable runtime home.

Blocked checks: macOS-only sandbox, Keychain, CoreAudio, and LaunchAgent behavior cannot be exercised on FreeBSD and remain covered on macOS.

## Progress

- [x] Native locked compilation passed.
- [x] Native descriptor-safe workspace and platform-name tests passed as `nobody`.
- [x] Full native suite failures were reproduced and classified.
- [x] Implement focused portability repairs.
- [x] Complete local and native validation gates.
- [x] Reconcile harness documentation and move this plan to completed.

## Decisions

- Decision: use `freebsd-x86-64` rather than raw `freebsd-x86_64` for signed artifact identity.
  Evidence: target validation intentionally accepts lowercase letters, digits, and hyphens only; existing release documentation uses `macos-aarch64`.
  Consequence: x86-family targets become representable without broadening the signed grammar.

- Decision: retain `required` as unavailable on FreeBSD.
  Evidence: the repository's documented security boundary is macOS `sandbox-exec`; FreeBSD uses explicit `policy-only` or `off`.
  Consequence: macOS-only execution tests are gated rather than weakening production checks.

## Surprises and Discoveries

- The `bsd` SSH account is root; filesystem-boundary tests therefore need an explicit `nobody` execution step.
- FreeBSD packages Python and Git under `/usr/local/bin`, while several fixtures and scheduled Git code assumed `/usr/bin`.

## Recovery and Rollback

All source edits are ordinary patch hunks. Revert only the files listed in Scope if requested; do not reset the dirty worktree. Disposable BSD paths can be removed after validation.

## Completion Record

Changed files: `src/agent/{client_tests.rs,tools.rs}`, `src/{external_runtime.rs,self_update.rs,schedule_v2.rs,lsp.rs,local_models.rs,ui_settings.rs,voice.rs,main.rs,service.rs}`, `tests/cli.rs`, and the BSD harness pages named in Scope.

Commands and results: `cargo fmt --check`, `cargo test --locked`,
`cargo clippy --locked --all-targets -- -D warnings`, and
`cargo build --locked --release` passed locally. Equivalent native FreeBSD
gates passed from `/tmp/ca-freebsd.audit2.dkElkM`; the released binary reported
`ca 0.1.0`, was a FreeBSD 15.1 x86-64 ELF, and hashed to
`27d89aef4db2f372961cbca13b4a516b97a0473d146af2794690267c8eaf0c3a`.

Remaining risks: no real provider, mailbox, installed-binary, Keychain,
CoreAudio, LaunchAgent, or mandatory FreeBSD sandbox check was run. Those are
outside this audit or unavailable by documented design.

Harness updates: `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, and
`TESTING.md` record the current FreeBSD boundary and validation evidence.
