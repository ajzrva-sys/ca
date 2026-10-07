# LSP-119 — Post-Edit Diagnostics

Status: COMPLETE (2026-08-16)
Owner/session: Codex / current reliability continuation
Baseline: `ae107301999744d490f1c134e9925f4d4c125011` plus completed uncommitted OBS-117 and INI-118

## Goal and Result

Synchronize configured language servers after successful text mutations and
append bounded diagnostics matching the new document version without ever
turning a successful mutation into a failure. The result covers model-driven
text mutations, `/init`, `/undo`, and `/redo`; deletion closes the document and
clears retained diagnostics.

## Boundaries and Decisions

- `publishDiagnostics` is retained process-only by configured server and
  canonical document path with its optional version.
- UTF-8 writes use `didOpen`/`didChange` and accept only the exact new version.
  Missing files use `didClose` and canonical-parent identity so macOS path
  aliases do not lose the open document.
- Reports include severity, source/code, one-based range, and sanitized message
  and explicitly label unavailable, stale, unversioned, timeout, failure,
  closure, and truncation states.
- Each mutation is capped at 50 diagnostics and exactly 32 KiB. The final
  truncation marker reserves its own bytes.
- Diagnostics are appended to the successful result and preserve the exact
  undo receipt. No LSP failure can fail the mutation.
- No server installation, automatic configuration, provider request, session
  schema, or background scan was added.

## Characterization and Discoveries

Characterization covered matching and stale versions, unversioned
notifications, terminal controls, deletion, strict item/byte limits, success
and undo-receipt preservation, no-LSP behavior, `/undo`, and wide/narrow pager
rendering. It found two implementation details: missing documents must be
identified through their canonical parent on macOS, and a truncation marker
must be included within—not after—the 32-KiB budget.

During native validation, one disposable re-transfer incorrectly included
macOS AppleDouble companion files; the experiment suite correctly rejected the
altered fixture manifests. That source tree was removed, the archive was made
metadata-free, and the exact-source rerun passed. A shared target cache also
retained an old compile-time manifest path for the experiment binary, so the
affected target was deliberately rebuilt in the clean disposable source.

## Validation

- Focused local: 5 `post_edit`, 11 `lsp`, targeted undo integration, and the
  reviewed wide/narrow snapshots pass.
- Local completion: 1,393 default main + 36 CLI; 1,422 all-feature main + 118
  experiment + 36 CLI; documented ignored cases only.
- Local formatting, strict all-feature Clippy, all-feature release,
  help/version, diff, and snapshot hygiene pass.
- Manual: disposable loopback TUI + fake LSP displayed one exact version-1
  warning after a successful write and carried it into the model continuation.
- Physical FreeBSD 15.1 amd64: focused 5/13; 1,380 default main + 36 CLI;
  1,409 all-feature main + 116 experiment + 36 CLI; native formatter, strict
  Clippy, release, help/version, ELF, and linkage pass.
- All disposable local/FreeBSD task trees were removed. User runtime state,
  credentials, binaries, oracles, and references were untouched.

## Recovery

The feature is process-only and additive. If rollback is required, remove the
diagnostic retention/synchronization helpers and their three integration
points while preserving the pre-existing six explicit LSP navigation tools
and all prior OBS-117/INI-118 work.
