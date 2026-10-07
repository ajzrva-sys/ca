# ART-224 — Session Artifact Ledger and Browser

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Persist a bounded metadata-only ledger of successful session file accesses and
provide a safe `/artifacts` browser without copying, deleting, or implicitly
trusting the referenced files.

## Non-Goals

- No file contents in the ledger, automatic artifact copying, underlying file
  deletion/archive, portable-export disclosure, uncontained viewing, or read
  tracking unless explicitly configured.
- No model authority, permission bypass, symlink following, or eager session
  rewrite.

## Evidence and Baseline

- `src/undo.rs` and tool execution results own exact successful mutation
  receipts; `src/session.rs` owns saved-state boundaries, forks, and rewinds.
- `src/workspace.rs` and read tools own containment/symlink/size safeguards.
- ORC-218 baseline: 868 records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Add defaulted `ca.session-artifacts.v1` state capped at 512 entries,
  coalesced by workspace-root identity, relative path, and access kind.
- Record successful create/modify/delete receipts automatically and successful
  reads only when `sessions.track_reads=true`. Store no content: only root ID,
  relative path, access/count, bounded turn/message reference, timestamps,
  size, and content/receipt digest.
- `/artifacts` provides bounded search/list/metadata, path copy, contained text
  view through existing safeguards, and ledger-only untracking. Every view
  revalidates root/path containment, symlink state, existence, and digest.
- Full forks retain entries through their message boundary; historical forks
  and rewinds discard later entries. Portable exports omit the ledger.

## Validation

Add failure-first mutation/read/cap/coalescing/containment/drift/fork/rewind/
export/untrack tests plus wide/narrow browser snapshots. Run focused session,
undo, tool, command, and rendering suites, formatting, strict Clippy, release,
and hygiene checks without provider traffic or installation.

## Progress

- [x] Child plan and queue entry created after SYS-223 completion.
- [x] Ledger schema and capture tests added.
- [x] Session/fork/rewind/export semantics implemented.
- [x] `/artifacts` browser implemented and reviewed.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Record

Implemented `src/session_artifacts.rs`, the defaulted session/config fields,
authoritative mutation/read/undo capture, boundary-aware branching, portable
omission, `/artifacts` list/search/inspect/view/copy/untrack, and reviewed
wide/narrow snapshots. Entries contain metadata and digests only; every view
revalidates the active root, path components, type, size, and digest.

Focused artifact coverage passes 13/13 plus the configuration and mutation
regressions. The first serial workspace run passed every non-CA target but
found two deterministic command fixture changes; both were corrected, and the
complete CA binary target then passed 2,341 tests with four documented ignores.
`cargo fmt --all -- --check`, strict workspace all-target/all-feature Clippy,
and the all-feature workspace release build pass. Reviewed snapshots contain
no pending `.snap.new` files. No provider traffic, user runtime mutation,
protected-tree change, oracle/reference edit, or installation occurred.

## Recovery

Reverse only ART-224-labelled hunks and delete only ART-created paths with
`apply_patch`. Never reset, clean, mutate oracle/reference trees, contact a
provider, alter user runtime state, or install a binary.
