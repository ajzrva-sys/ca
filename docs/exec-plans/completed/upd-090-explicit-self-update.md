# UPD-090 — Explicit self-update

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Add an explicit `ca update` lifecycle that pins one exact Ed25519 authority, fetches only a requested signed release manifest, stages one exact verified current-target binary, atomically replaces only the invoking executable after exact confirmation, retains one verified rollback copy, and persists explicit channel/pin choices without any automatic check or install.

## Non-Goals

- Background checks, TUI notices, install scripts, archives, package managers, privilege escalation, PATH discovery, multiple mirrors, or automatic installation.
- Updating any binary other than the exact non-symlink `current_exe`, or touching the repository/installed binary during validation.
- Claiming Ante or Cursor parity.

## Evidence and Current Behavior

- Ante documents explicit update, exact-version rollback, stable/nightly channels, and persisted channel selection. This is documentation evidence only; its installer-script implementation is not adopted.
- EXT-088 already establishes exact HTTPS/literal-loopback authority, Ed25519 envelopes, no-proxy/no-redirect downloads, owner-only state, streaming hash verification, and atomic retained bytes.
- `src/main.rs` reserves leading service/manager commands while preserving positional prompts. There is no self-update boundary today.

## Scope

Allowed: a new `src/self_update.rs`, `src/main.rs`, `src/runtime.rs`, focused CLI tests, this plan, and standard harness docs.

Forbidden: oracle/reference edits, real user state, installed binaries, system package managers, privileged paths, or external services.

## Design and Invariants

- Leading `ca update` exposes trust/status/check/channel/pin/download/install/rollback. Persistent actions require exact confirmations where authority or executable bytes change; bare/profile selection is not part of this global binary lifecycle.
- Trust pins one exact HTTPS base path—or literal-loopback HTTP in fixtures—and one local Ed25519 key. Check fetches only `releases.json`, verifies exact signed bytes, monotonic revision, closed channel/version/current-target metadata, and persists no response body or URL in safe output.
- Download resolves only the signed same-origin artifact, streams through exact byte/SHA-256 caps into private staging, rechecks it, and retains it without execution. Channel resolution uses the persisted channel unless an exact pin is present; explicit version overrides both.
- Install uses only an already staged verified release, revalidates the current executable and target parent, creates a verified owner-only rollback copy, writes one private same-directory replacement, fsyncs, and atomically renames over exactly `current_exe`. Failure keeps/restores the original.
- Rollback revalidates the retained backup and atomically restores it after exact confirmation. One active rollback copy is retained; no automatic cleanup/update occurs.
- Tests invoke a copied release binary or injected fake executable under temporary roots only.

## Milestones

1. Characterize leading-command, URL/signature/manifest/state, and no-write status behavior.
2. Implement explicit check/channel/pin/stage/install/rollback with safe failure cleanup.
3. Run focused/full/release gates, disposable copied-binary lifecycle, reconcile docs, and archive.

## Acceptance Criteria

- No network before explicit check/download and no binary write before exact confirmed install/rollback.
- Only signed current-target exact bytes can be staged; redirects, proxies, URL credentials/query/fragment, traversal, mismatches, stale revisions, bodies, symlinks, and widened modes fail closed.
- Install affects exactly the invoking copied binary in tests, retains a verified rollback copy, and rollback restores exact bytes.
- Full gates pass without touching the repository release binary or any installed/user state.

## Validation

- Focused `cargo test self_update --no-fail-fast` and update parser/CLI tests.
- Full test, fmt, strict Clippy, release build, help/version/update smokes.
- Released copied-binary plus generated key/signed literal-loopback fixture only.

## Progress

- [x] Verify oracle, signed-runtime, CLI, and current-executable boundaries.
- [x] Add characterization tests and manager state.
- [x] Implement signed check/stage/install/rollback.
- [x] Run full gates and copied-binary walkthrough.
- [x] Reconcile docs and archive.

## Decisions

- Decision: require a staged verified artifact before install.
  Evidence: separating network from executable replacement makes the disk mutation previewable and retryable without changing authority.
  Consequence: `check`, `download`, and `install` are distinct explicit operations.
- Decision: target only `current_exe` and test through a copied released binary.
  Evidence: a public arbitrary target path would create a general file-replacement primitive.
  Consequence: repository validation can prove the real CLI without modifying the repository or installed executable.

## Surprises and Discoveries

- The first disposable manifest used Rust's target triple while the closed manifest contract intentionally uses `macos-aarch64`; the invalid manifest was rejected before download. The corrected clean walkthrough made exactly one manifest and one artifact request.

## Recovery and Rollback

All downloads use private UUID staging; replacement uses a private create-new sibling plus atomic rename. The original is hashed into one private rollback record before replacement. No broad deletion or Git recovery is used.

## Completion Record

Changed files: `src/self_update.rs`, `src/main.rs`, `src/runtime.rs`, `tests/cli.rs`, and the standard harness documents.

Commands and results:

- `cargo test self_update --no-fail-fast`: PASS, 9 focused tests.
- `cargo test update_help_and_status_are_local_and_do_not_create_state --test cli --no-fail-fast`: PASS.
- `cargo test --no-fail-fast`: PASS, 1085 unit tests and 36 CLI integration tests; one manual browser test remained ignored by the ordinary gate.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- Released help/version/update-status smoke checks: PASS under a disposable home, with no state creation for status.
- Released copied-binary walkthrough: PASS. A generated Ed25519 authority served exactly `GET /releases.json` and `GET /artifacts/ca-0.1.1-fixture`; explicit trust/check/channel/pin/download/install/rollback restored the copy byte-for-byte, retained owner-only state, left the repository release hash unchanged, and created no config or session.

Remaining risks: release publication and platform signing/notarization are external operational responsibilities; the manager has no built-in trust root or automatic network behavior. Authority rotation is intentionally refused pending a separate recovery design.

Harness updates: architecture, runtime paths, parity scope, validation results, continuity, knowledge index, and next steps now record UPD-090 as complete.
