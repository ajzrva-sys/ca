# INP-131 — Explicit Prompt Stash

Status: COMPLETE
Owner/session: Codex / current user-requested implementation
Last updated: 2026-08-16

## Goal

Add an explicit, profile-scoped, bounded composer draft stash with safe
restore/swap semantics and no implicit persistence of ordinary input history.

## Non-Goals

- Do not persist drafts without an explicit save or restore/swap action.
- Do not grant restored attachments authority before ordinary send-time
  validation.
- Do not write in bare mode, add a drawer, or implement later roadmap tasks.

## Evidence and Current Behavior

- `src/prompt_stash.rs` owns strict versioned storage and all count, byte,
  permission, symlink, corruption, and atomic-write boundaries.
- `src/tui/input.rs` owns picker navigation, restore/swap, two-step deletion,
  and configurable `Ctrl+X S` dispatch.
- `src/main.rs` is the only orchestration boundary that clears the composer or
  typed attachment state after successful storage operations.
- Restored `@` text reaches the existing `expand_file_mentions` send path; an
  integration test deletes the referenced file after restore and observes the
  expected unavailable marker rather than stale authority.

## Scope

Allowed files: prompt-stash storage, app/TUI/keymap/command orchestration,
deterministic snapshots, and repository harness documents.

Forbidden files/areas: `oracles/`, `reference/`, user runtime state, installed
binary, provider protocols, session schemas, and later roadmap work.

Runtime state affected: explicit profile writes beneath
`input/prompt-stash.json`; bare mode has no store.

## Design and Invariants

- `/stash`, `/stash save`, and configurable `stash_draft` share typed action
  paths. No normal history path invokes the store.
- At most 50 entries, 64 KiB per entry, 1 MiB aggregate text, and 8 MiB
  serialized input are accepted.
- Owner-controlled directories/files are 0700/0600. Atomic create-new temp,
  sync, and rename publish complete documents.
- Home is canonicalized so platform-owned ancestors such as macOS `/tmp` are
  accepted; every profile-owned path component remains no-symlink.
- A corrupt store is not rewritten by reads. The next explicit save moves the
  valid owner-controlled regular file to a bounded hidden quarantine name and
  publishes a fresh store.
- Restore removes the selected entry and atomically saves a non-empty current
  composer in the same transaction. Pasted images are refused.
- Stored attachments are inert text; typed attachment objects are cleared and
  send-time validation re-resolves current file contents.

## Milestones

### M1 — Storage and command boundary

- Complete: added validated storage, caps, corruption recovery, profile safety,
  and explicit command/action surfaces.

### M2 — Input and presentation

- Complete: added chord, picker navigation, restore/swap, repeated deletion,
  bounded previews, deterministic age/line count, and wide/narrow snapshots.

### M3 — Validation and completion

- Complete: proved attachment drift, ran full gates/native PTY, reconciled the
  harness, and activated CFG-132.

## Acceptance Criteria

- Explicit save is the only draft-creation path: PASS.
- Restore removes the selected entry and atomically swaps non-empty input:
  PASS.
- Bare, symlinked, widened, corrupt, oversized, and pasted-image cases fail
  without silent data loss: PASS.
- Restored attachment text revalidates at send time: PASS.
- Picker and keymap render deterministically at wide/narrow sizes: PASS.

## Validation

Focused:

- `cargo test prompt_stash`: PASS, 9 tests.
- `cargo test stash_is_an_explicit`: PASS.
- `cargo test prompt_stash_commands_swap_text`: PASS.

Completion:

- `cargo test`: PASS, 1,507 main and 37 CLI tests; one ignored.
- `cargo test --all-targets --all-features`: PASS, 1,536 main, 118 experiment,
  and 37 CLI tests; two ignored.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo build --release --all-features`: PASS.
- Release help/version, `git diff --check`, and snapshot hygiene: PASS.

Manual/visual:

- Released native TUI under disposable home
  `/tmp/ca-inp131-pty-20260816` proved chord save, preview, restore, and
  two-step deletion. The first run exposed macOS `/tmp` ancestry and drove the
  corrected canonical-home/profile-relative symlink rule. The successful
  fixture moved recoverably to
  `/Users/aji/.Trash/ca-inp131-pty-20260816`.

Blocked checks: physical FreeBSD was not requested or run.

## Progress

- [x] Storage/config/input seams characterized.
- [x] Bounded explicit persistence and swap semantics complete.
- [x] Picker, keymap, commands, and snapshots complete.
- [x] Native walkthrough, full gates, and harness complete.

## Decisions

- Decision: store only composer text and creation time.
  Evidence: typed attachment authority and pasted images are process-only.
  Consequence: restore is safe and send-time validation remains authoritative.
- Decision: quarantine corruption only on explicit save.
  Evidence: opening `/stash` is a read and must not create or mutate storage.
  Consequence: inspection is side-effect-free while explicit recovery remains
  available.
- Decision: validate symlinks below canonical home rather than from `/`.
  Evidence: macOS `/tmp` is a system symlink to `/private/tmp`.
  Consequence: disposable and legitimate homes work while profile-owned swaps
  remain rejected.

## Surprises and Discoveries

- Native validation showed that checking every filesystem ancestor for
  symlinks rejected valid macOS temporary homes. The bounded security property
  applies to user-controlled profile components, not trusted system aliases.

## Recovery and Rollback

The store is additive profile data and no normal input history is migrated.
Rollback can stop reading the file without rewriting sessions or config. Never
use destructive Git reset/checkout for recovery.

## Completion Record

Changed files: `src/prompt_stash.rs`, `src/keymap.rs`, `src/app.rs`,
`src/tui/commands.rs`, `src/tui/input.rs`, `src/tui/render.rs`, tests/snapshots,
`src/main.rs`, and harness documents.

Commands and results: all focused/default/all-feature/format/strict-lint/
release/smoke/diff/snapshot gates above passed.

Remaining risks: physical FreeBSD was not exercised for this task; persistent
operations are deliberately synchronous but bounded to the small store limits.

Harness updates: architecture, continuity, testing evidence, executable queue,
and documentation index now record completion and CFG-132 activation.
