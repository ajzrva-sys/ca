# WSP-096 — Copy-on-Write Workspace Materialization

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-12

## Goal

Provide one reusable, verified disposable-workspace boundary for hermetic benchmarks and isolated scheduled mutations. APFS cloning is an optimization; source/destination manifests define correctness and ordinary copying is an equivalent fallback.

## Non-Goals

- APFS volume snapshots, privileged helpers, volume rollback, or main-checkout mutation.
- Copying Git administrative state, submodules, secrets, runtime configuration, sessions, caches, or build output.
- Materializing a dirty current checkout or integrating/committing/pushing generated changes.
- Wiring the materializer into the experiment or scheduler before this primitive is complete.

## Evidence and Current Behavior

- `src/worktree.rs` creates exact Git worktrees but does not provide a non-Git disposable materialization boundary.
- `src/schedule.rs` currently permits scheduled work in a configured workspace; later SCH-099 will require this primitive for mutating runs.
- Apple documents file cloning as a same-filesystem copy-on-write operation. Directory cloning is discouraged, so this implementation walks a verified tree and clones regular files individually.
- The repository has no committed baseline and all current files are user-owned; tests use only temporary directories.

## Scope

Allowed files:

- `src/workspace_materialization.rs`
- `src/main.rs` module registration
- `docs/exec-plans/active/wsp-096-cow-workspace-materialization.md`, later moved to `completed/`
- `NEXT_STEPS.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`

Forbidden files/areas:

- `oracles/`, `reference/`, user configuration/session/Keychain state, installed binaries, and unrelated runtime modules.

Runtime state affected:

- None unless a caller explicitly materializes into its supplied private root. Tests use temporary directories and clean them.

## Design and Invariants

- A source is a clean fixture tree or tracked content from a separately created pinned worktree. Every `.git` file/directory is excluded; the destination is intentionally not a Git repository.
- Scan a deterministic, bounded source manifest before materialization, materialize only that manifest, rescan the source, and independently hash the destination. Any drift fails and removes the owned disposable directory.
- Regular files use macOS `clonefile` with no-follow semantics when possible. Unsupported/cross-filesystem cloning falls back to create-new bounded copying. Report `all_cloned`, `mixed`, or `copied`.
- Benchmark mode rejects all symlinks. Scheduled mode requires an exact normalized tracked-path allowlist and accepts only relative links resolving to an included in-root target; special files and regular hardlinks always fail.
- Defaults are 100,000 entries, 2 GiB logical content, 5 GiB minimum free space, and two concurrent materializations.
- Source/private/staging/published trees are held through no-follow directory descriptors; `openat`/`renameat`/`unlinkat` operations remain bound to exact identities across path swaps. Only unpublished staging is RAII-cleaned. A published workspace survives handle drop or cancellation, has an owner-only durable receipt/reopen path, and is removed only by explicit retryable cleanup after a bounded external artifact outside the private root is bound to its exact ID and baseline/final manifests.
- The two-materialization gate is process-local in WSP-096. SCH-099 must add root-scoped cross-process leases/reservations before standalone workers use it.
- The source walker has a bounded depth, rejects empty/no-regular-file inputs, and never labels a vacuous tree `all_cloned`.

## Milestones

### M1 — Characterization and data contract

- Add bounded manifest, policy, receipt, and lifecycle types with copy-only tests.
- Validate exclusions, quotas, special files, symlink modes, deterministic hashes, and cleanup.

### M2 — CoW and race containment

- Add macOS regular-file cloning with fallback and outcome accounting.
- Compare source-before/source-after/destination manifests and fail closed on drift.

### M3 — Validation and harness reconciliation

- Run focused and repository completion gates.
- Record exact results, update current docs, and archive only when acceptance passes.

## Acceptance Criteria

- Copy-only and clone-preferred materializations produce an exact destination manifest.
- No `.git` pointer/object or closed excluded state appears in the destination.
- Symlink/special-file/quota/free-space failures leave no owned workspace behind.
- A source mutation between snapshots is detected.
- macOS same-volume APFS materialization proves cloning when available; portable copying remains correct without it.
- Cleanup cannot target anything outside the materializer-owned temporary directory.

## Validation

Focused:

- `cargo test workspace_materialization --no-fail-fast`
- macOS clone-preferred characterization and forced-copy tests

Completion:

- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`

## Progress

- [x] Establish exact scope and safety contract.
- [x] Add failing focused characterizations and public data types.
- [x] Implement bounded verified copy/clone materialization.
- [x] Complete focused and repository validation.
- [x] Reconcile documentation and archive the plan.

## Decisions

- Decision: never copy `.git`, including linked-worktree pointer files.
  Evidence: copied pointer files would still reference the original common repository and could mutate its Git metadata.
  Consequence: later callers create a pinned worktree separately and materialize tracked content into a deliberately non-Git directory.

- Decision: use per-file cloning rather than directory `clonefile`.
  Evidence: the macOS API discourages directory cloning and offers no correctness advantage over a verified per-entry walker.
  Consequence: every file participates in quotas, outcome accounting, and destination hashing.

## Surprises and Discoveries

- The absence of a committed repository baseline makes broad Git diff ownership unavailable; changes and tests must stay exact and temporary.
- Path-based rescan and cleanup were insufficient under same-user rename/symlink races. The implementation therefore holds root/child descriptors throughout and verifies exact device/inode/type before every mutating cleanup action.
- A caller-provided digest was not cleanup authority. WSP now persists a durable receipt and mints a non-serializable recovery capability only after binding an external owner-only artifact to the exact workspace and manifests.

## Recovery and Rollback

Remove the new module registration and module with targeted patches, then remove this plan entry. Disposable test/workspace directories are owner-created and self-cleaning. Do not use destructive Git commands.

## Completion Record

Changed files:

- `src/workspace_materialization.rs`
- `src/main.rs`
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`
- this ExecPlan, archived under `docs/exec-plans/completed/`

Commands and results:

- `cargo test workspace_materialization --no-fail-fast`: PASS, 22/22.
- `cargo test --no-fail-fast`: PASS, 1152 unit tests, one ignored, and 36 CLI tests.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets -- -D warnings`: PASS.
- `cargo build --release`: PASS.
- released `--help`, `--version`, and snapshot hygiene: PASS (`ca 0.1.0`, zero `.snap.new`).

Remaining risks:

- Cross-process/root-scoped leasing and exact Git tracked-list generation belong to SCH-099; WSP-096 admission is process-local.
- Non-Unix callers fail before mutation. The accepted benchmark/scheduler path is macOS/Unix and no privileged APFS volume snapshot API is used.
- The recovery capability proves artifact identity/stability and workspace binding; the caller remains responsible for format-specific proof that a retained patch/archive can reconstruct the final manifest.

Harness updates:

- Current architecture, continuity, test evidence, source index, and queue now describe the shipped boundary and explicit SCH-099 handoff.
