# EDT-226 — ACP IDE Context and Hash-Anchored Editing

Status: COMPLETE
Owner/session: ORC-218 goal session
Last updated: 2026-08-29

## Goal

Extend stable ACP v1 prompts with bounded explicit editor resources and add
host-verified line anchors plus an approval-preserving `hashline_edit` tool.

## Non-Goals

- No ACP v2/private wire extension, editor-state discovery, automatic workspace
  context, binary media, symlink following, provider traffic, runtime-state
  mutation, oracle/reference edit, or installation.
- Anchors grant no authority and do not bypass approval, permission, sandbox,
  undo, mutation receipts, loop guards, or the artifact ledger.

## Evidence and Baseline

- `src/acp.rs` already implements the stable official SDK v1 session surface
  and treats content blocks as explicit prompt data.
- `read_file` and existing mutation tools already centralize workspace,
  approval, sandbox, undo, receipt, loop-guard, and artifact paths.
- ORC-218 baseline: 868 records, SHA-256
  `2dcaf84dc86d3ae27bfea593bc811ce16f46f9782382780901fdcf890ff39978`.

## Design

- Accept at most 32 explicit text embedded resources or workspace-contained
  `file:` resource links, at most 64 KiB each and 128 KiB aggregate. Reject
  non-file links, binary/unsupported media, path escape, symlinks, unavailable
  files, malformed ranges, and oversize before provider construction. Render
  visible per-resource provenance into only that user prompt.
- `read_file.include_line_hashes` adds stable eight-hex content anchors per
  returned line without changing the default output. Anchors bind exact line
  bytes and ordering, including Unicode and CRLF input.
- `hashline_edit` rereads the contained regular file and requires unique,
  contiguous start/end anchors. It rejects stale, missing, duplicate,
  noncontiguous, or mismatched anchors before mutation, then delegates through
  CA's existing file-edit execution and receipt path.

## Validation

Add failure-first ACP SDK fixtures for resources, bounds, ranges, containment,
and links; add anchored-read/edit tests for Unicode, CRLF, collisions, stale
bytes, approval, undo, receipt, loop guard, and artifact integration. Run
focused suites, formatting, strict all-feature Clippy, release, and hygiene
checks without provider traffic or installation.

## Progress

- [x] Child plan and queue entry created after PAN-225 completion.
- [x] Stable ACP resources implemented and bounded.
- [x] Line anchors and `hashline_edit` implemented through existing authority.
- [x] Focused suites and child completion gates pass.
- [x] Harness reconciled and plan completed.

## Completion Evidence

- Stable ACP v1 advertises embedded context and accepts only bounded text
  embedded resources or contained regular `file:` links: at most 32 items,
  64 KiB each, and 128 KiB aggregate. Unsupported media, remote schemes,
  malformed ranges, traversal, symlinks, binary text, and oversize fail before
  provider construction. Official SDK new/load/prompt/mode coverage passes.
- `read_file.include_line_hashes` emits optional eight-hex content anchors
  without changing default output. `hashline_edit` requires unique ordered
  anchors and flows through normal approval, workspace/sandbox, undo, mutation
  receipt, loop guard, LSP, activity, and artifact-ledger handling. Unicode,
  CRLF, stale, duplicate, reversed, malformed, receipt, undo, and ledger cases
  pass.
- Focused ACP tests pass 12/12; focused anchored-edit and schema tests pass. The
  complete serial CA target passes 2,359 tests with four documented ignores.
  Formatting, strict CA all-feature Clippy, release, diff/snapshot hygiene, and
  direct oracle/reference inspection pass. The existing macOS `__eh_frame`
  linker warning remains non-fatal. No provider request, runtime/profile
  mutation, protected-tree/oracle edit, or installation occurred.

## Recovery

Reverse only EDT-226-labelled hunks and delete only EDT-created paths with
`apply_patch`. Never reset or clean, mutate oracle/reference or protected
trees, contact a provider, alter user runtime state, or install a binary.
