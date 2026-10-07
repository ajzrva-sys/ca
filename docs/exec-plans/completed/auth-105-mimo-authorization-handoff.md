# AUTH-105 — MiMo Authorization Handoff Repair

Status: COMPLETE
Owner/session: Codex implementation session
Last updated: 2026-08-15

## Goal

Make the FreeBSD MiMo sign-in handoff produce Xiaomi's direct published
`/authorize` URL without terminal UI characters, while retaining a reliable
clipboard request and a borderless selectable fallback.

## Non-Goals

- Changing MiMo's X25519/AES-GCM credential contract.
- Inventing a different Xiaomi endpoint or modifying the read-only oracle.
- Reading, printing, or persisting a real credential during automated tests.

## Evidence and Current Behavior

- The user's captured URL contains `%E2%94...`; Xiaomi reports an illegal `%`
  escape whose input begins with `│`, the provider modal's border character.
- Before AUTH-105, `src/mimo_auth.rs` wrapped the published authorization URL in
  `/api/v1/genLoginUrl?currentPath=...`.
- The current MiMoCode source in
  `oracles/MiMo-Code-main/packages/opencode/src/plugin/mimo.ts` builds the direct
  `/authorize` URL.
- MiMoCode's CLI prints the complete URL as raw terminal text, and its clipboard
  helper wraps OSC 52 for tmux/screen passthrough.

## Scope

Allowed files: `src/mimo_auth.rs`, `src/main.rs`, provider-auth TUI tests/copy,
snapshots, and AUTH-105 harness documentation.

Forbidden files/areas: `oracles/`, `reference/`, credentials, provider config,
sessions, and unrelated dirty-tree changes.

Runtime state affected: none until the rebuilt binary is explicitly run by the
user; tests use generated ephemeral keys only.

## Design and Invariants

- Xiaomi receives the exact direct published authorization contract.
- Authorization URLs are validated for size/control characters before terminal
  output.
- Remote handoff leaves the alternate screen and emits one raw logical URL line
  without borders; terminal soft wrapping must not change its bytes.
- Clipboard output retains OSC 52 and adds the MiMoCode tmux/screen passthrough.
- No authorization result or provider credential enters snapshots or logs.

## Milestones

### M1 — Characterize the regression

- Change the URL fixture to require direct `/authorize` output.
- Add deterministic handoff/clipboard tests that reject controls and prove no
  border character or hard line break enters the URL.

### M2 — Implement the source-aligned handoff

- Remove the undocumented login-wrapper construction.
- Present the FreeBSD URL outside the bordered TUI automatically and retain a
  repeatable Ctrl+Y handoff.
- Add tmux/screen OSC 52 passthrough.

### M3 — Validate and install

- Run focused tests, snapshot review, full tests, formatting, strict all-target
  Clippy, and release builds on macOS and FreeBSD.
- Install only the verified platform-native binaries in the existing locations.

## Acceptance Criteria

- A generated MiMo URL begins with
  `https://platform.xiaomimimo.com/authorize?` and decodes all four published
  parameters.
- The raw handoff contains the URL as one exact logical line with no `│`.
- FreeBSD automatically shows that borderless handoff; Ctrl+Y can show it again.
- Existing MiMo crypto/callback fixtures and provider-auth snapshots pass.

## Validation

Focused: PASS. Four MiMo URL/crypto/callback tests and five provider-auth
handoff/input/render tests passed on macOS and physical FreeBSD.

Completion: PASS. macOS passed 1,269 applicable unit tests, one accepted manual
ignore, all 36 CLI tests, formatting, strict all-target Clippy, release build,
and help/version smokes. Physical FreeBSD passed 1,256 applicable unit tests,
the same manual ignore, all 36 CLI tests, native formatting, locked strict
all-target Clippy, release build, and help/version/ELF/linkage smokes.

Manual/visual: PASS. Reviewed wide/narrow snapshots with the long URL removed
from the bordered popup. A native FreeBSD PTY walkthrough automatically emitted
the direct Xiaomi URL as one borderless logical line, requested OSC 52, and was
canceled before credential entry or configuration mutation.

## Progress

- [x] Captured the screenshot failure and current MiMoCode behavior.
- [x] Added failing direct-URL and terminal-handoff characterization.
- [x] Implemented direct URL, borderless fallback, and passthrough clipboard.
- [x] Completed macOS and FreeBSD validation and installation.

## Decisions

- Decision: treat the terminal border contamination as the reproduced failure.
  Evidence: Xiaomi's error names `│`, and the captured URL visibly contains its
  UTF-8 percent encoding immediately after `pk=M`.
  Consequence: the fix must make manual selection independent of the popup.
- Decision: keep the long URL out of the bordered popup.
  Evidence: mouse selection of the wrapped popup was the exact contamination
  path; FreeBSD now shows a raw handoff automatically and Ctrl+Y repeats it.
  Consequence: the modal advertises the borderless handoff but cannot contribute
  box-drawing bytes to a selected URL.

## Surprises and Discoveries

- The FreeBSD account's rustup `cargo-fmt` shim is not a native executable; the
  packaged `/usr/local/bin/cargo-fmt` and `cargo-clippy` completed the native
  gates without changing the host toolchain.

## Recovery and Rollback

Reverse only the AUTH-105 scoped patch; do not reset the dirty worktree. The
installed binaries can be replaced by a prior verified platform-native build.

## Completion Record

Changed files: MiMo URL construction/tests; provider-auth orchestration,
clipboard/handoff helpers and tests; auth input/render/snapshots; harness docs.

Installed artifacts:

- macOS `/Users/aji/.local/bin/ca`, SHA-256
  `32a4f164f46109394e8e981bb9e1aba406692abd867f6d5553d97ffedecbac6c`.
- FreeBSD `/home/aji/bin/ca`, SHA-256
  `4cf052f74bb849e09e734566087bc88a7a9cccd584355acd067014ef5cade691`.

The exact disposable FreeBSD build directory was removed after installation.
No credential, provider configuration, session, oracle, or unrelated user file
was changed.
