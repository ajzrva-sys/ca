# DIA-219 — Support Bundles and Configuration Provenance

Status: COMPLETED
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Add secret-safe configuration provenance and a private, atomic, bounded support
archive through `ca config explain`, `/config explain`, `ca support-bundle`, and
`/support-bundle`, without retaining prompts, transcripts, raw paths, provider
payloads, tool content, environment values, or credentials.

## Scope and Invariants

- Add versioned `ca.config-provenance.v1` and `ca.support-bundle.v1` records.
- Provenance records only selector, effective source class, reference type, and
  present/absent state. Values and raw reference identifiers are excluded.
- Support archives are gzip-compressed tar files with at most 8 MiB of
  uncompressed regular-file content. Entries are generated in memory from safe
  typed reports or copied only from the existing metadata-only log store.
- Output creation rejects symlinks, hardlinks, unsafe ownership/mode,
  collisions, and drift. Reservation and publication are private and atomic;
  failure removes only writer-owned staging.
- The manifest binds entry names, sizes, and SHA-256 digests plus safe
  version/platform/profile facts. No absolute path is serialized.
- CLI operations are local and provider-free. TUI archive creation is
  Cancel-first and previews exact entries and total uncompressed size.
- Preserve all pre-ORC dirty bytes and never modify `oracles/`, `reference/`,
  runtime configuration, sessions, credentials, or the installed binary.

## Milestones

1. Add a deterministic typed provenance projection for built-in defaults,
   explicit profile values, process overrides, environment references,
   Keychain references, and derived runtime values, with selector filtering.
2. Add a private support-bundle planner/writer with safe typed doctor/audit/
   runtime-health/log/provenance inputs, entry hashes, caps, and drift checks.
3. Wire exact CLI parsing/output and Cancel-first TUI preview/confirmation.
4. Add focused failure-first tests, reviewed wide/narrow snapshots, child gates,
   and reconcile the ORC harness before moving this plan to completed.

## Validation

- Focused provenance source/selector/redaction tests.
- Support archive permission, cap, manifest/hash, collision, drift,
  symlink/hardlink, atomic cleanup, log filtering, and secret-sentinel tests.
- CLI valid/conflict/JSON schemas and TUI wide/narrow preview tests.
- `cargo fmt --all -- --check`; strict all-feature Clippy; all-feature release.
- The two complete workspace matrices remain final ORC-218/REL-229 gates.

## Progress

- [x] Child plan and queue entry created after AGT-157 completion.
- [x] Provenance model and explain surfaces complete.
- [x] Support-bundle planner/writer complete.
- [x] CLI/TUI integration complete.
- [x] Focused tests and child gates pass.
- [x] Harness reconciled and plan completed.

## Recovery

Reverse only DIA-219-labelled hunks with `apply_patch`. Archive tests use
disposable roots; no user runtime path is mutated during validation.

## Completion Record

Changed files: new `src/config_provenance.rs` and `src/support_bundle.rs`;
focused additions to `src/runtime_diagnostics.rs`, `src/shell_integration.rs`,
`src/main.rs`, `src/app.rs`, and `src/tui/{commands,input,render,render_tests}.rs`;
direct `flate2`/`tar` dependencies; and four reviewed snapshots (two new
support previews and the expected one-line command-count updates in the two
pre-existing command-discovery snapshots).

Commands and results:

- Provenance tests passed 3/3; support-bundle tests passed 9/9; the runtime
  support-snapshot test, shell-integration suite (4/4), slash-command tests,
  and wide/narrow render tests passed.
- `cargo test --workspace --no-fail-fast -- --test-threads=1` completed after
  the expected command-palette snapshot update; the complete CA target rerun
  passed 2,289 tests with four documented ignores, and every other workspace
  target in the matrix had already passed.
- `cargo fmt --all -- --check`, strict workspace all-target/all-feature
  Clippy, and the all-feature release build passed.
- Release help passed for `ca support-bundle` and `ca config explain`;
  `ca --version` returned `ca 0.1.0`; a provider-free JSON explain smoke
  returned `ca.config-provenance.v1` with source class and presence only.
- `git diff --check`, zero `.snap.new`, and direct `oracles/`/`reference/`
  inspection passed.

Remaining risks: an actual archive was not written into the user's profile
during smoke testing; private publication, hashes, caps, collision/drift,
symlink/hardlink, cleanup, and redaction behavior are covered in disposable
fixtures. The release linker continues to emit the existing macOS oversized
`__eh_frame` warning without failing. Installation remains deferred to
REL-229.

Harness updates: the umbrella plan, queue, continuity, testing map, and index
record this completion. DIA-219 intentionally extended pre-existing CUR-owned
runtime-diagnostics, shell, and central TUI files while preserving their
baseline bytes and all unrelated dirty-tree work.
