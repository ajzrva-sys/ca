# DOC-066 — PDF Text Reading

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Add one local read-only `read_pdf` tool that extracts a bounded 1-based inclusive page range from a workspace PDF, reports truthful file/page/continuation metadata, rejects encrypted or malformed documents safely, performs parsing on a blocking worker, and never executes or extracts embedded content.

## Non-Goals

- OCR, image rendering, annotation/form editing, password entry, embedded-file extraction, JavaScript/actions, remote PDFs, or PDF generation.
- Provider-visible image content; that requires MM-067.
- Automatic external utility installation or invocation.
- Cursor parity. This is an oracle-informed `ca` extension.

## Evidence and Current Behavior

- `read_file` is UTF-8 text-only and has no PDF page semantics (`src/agent/tools.rs::read_file_range`).
- The central registry has no PDF tool or schema (`src/agent/registry.rs`).
- Workspace path preparation already canonicalizes read-only `Path` resources before approval/execution (`src/workspace.rs`).
- No PDF dependency or local `pdftotext` executable is available in the current build environment.
- `pdf-extract` 0.12.0 is MIT-licensed, pure Rust, reexports `lopdf`, exposes per-page `output_doc_page`, and supports an explicit encryption check. Its parser contains panic-prone malformed-object paths, so the worker must add a panic boundary.

## Scope

Allowed files: `Cargo.toml`, `Cargo.lock`, `src/agent/{registry,tools}.rs`, focused tests/fixtures/snapshots, this plan, and required harness documentation.

Forbidden: oracle/reference trees, user runtime state, external PDF tools/downloads, embedded-file/action execution, provider/network calls, and unrelated refactors.

Runtime state affected: none beyond bounded process memory during one tool call. Tests use generated PDFs in temporary workspaces.

## Design and Invariants

- Add `read_pdf { path, start_page?, end_page? }`; pages are 1-based inclusive, default to page 1, and span at most 20 pages.
- Accept regular `.pdf` files up to 32 MiB and parsed documents up to 10,000 pages. Read under existing workspace authority before moving owned bytes to `spawn_blocking`.
- Load from memory, reject encrypted documents without trying passwords, enumerate the exact total page count, and call `output_doc_page` only for requested pages.
- Wrap parser/extractor work in `catch_unwind`; malformed data, parser panics, worker failure, and encryption map to fixed secret/path-free messages.
- Feed `PlainTextOutput` through a UTF-8-preserving capped writer. Reserve result space for headers/metadata, stop extraction at 56 KiB, and return at most 64 KiB total. If capped, say remaining selected-page text was not extracted; never imply an exact omitted byte count.
- Prefix each emitted page, sanitize terminal controls, and report shown range, total pages/file bytes, pages omitted before/after, output-cap status, and an exact next-range JSON hint when later pages remain.
- The tool is read-only and available wherever `read_file` is currently available (Main, Subagent, Review). It does not gain network/process/mutation authority.
- Never traverse `/Names`, `/EmbeddedFiles`, annotations, actions, JavaScript, or attachments; page content text extraction is the only operation.

## Milestones

### M1 — Failing contract tests

- Add a failing registry/schema test and generated two-page fixture tests for default/ranged behavior.
- Validate the failure is only the missing PDF tool.

### M2 — Bounded extraction

- Add the dependency, capped writer, blocking parser, page-range validation, safe failures, truthful metadata, and tool dispatch.
- Test Unicode text, range/default/continuation, oversized file/span/page count, encrypted/malformed/panic containment, output cap, and terminal sanitization.

### M3 — Runner/render coverage

- Verify central workspace/approval/profile behavior and headless execution.
- Add reviewed wide/narrow success/failure tool-card snapshots.

### M4 — Gates and reconciliation

- Run focused suites, full tests, formatting, strict Clippy, release build, smoke, and a released fake-provider walkthrough against a generated temporary PDF.
- Reconcile harness docs and archive this plan before MM-067.

## Acceptance Criteria

- Default reads only page 1 and tells the model how to request page 2.
- Explicit ranges are exact and never exceed 20 pages or the document bounds.
- Output never exceeds 64 KiB and labels extraction-cap truncation honestly.
- Encrypted, malformed, oversized, out-of-workspace, wrong-extension, and parser-panic inputs fail safely without paths or parser internals.
- Only selected page content is processed; embedded files/actions are never opened or executed.
- Parsing does not block the async runtime worker.
- Existing tools/config/sessions remain compatible and no external executable is required.

## Validation

Focused: PASS. Registry, generated-PDF tool, page/file/output-cap, encryption/malformed/panic, workspace, terminal-safety, embedded-content, and render tests all passed.

Completion: PASS. `cargo test --no-fail-fast` (754 + 28), `cargo fmt --check`, strict Clippy, release build, help/version/profile smoke, and zero `.snap.new` files.

Manual/visual: PASS. Four wide/narrow snapshots were reviewed; a released headless loopback run extracted only page 2 from a generated two-page PDF and completed its tool continuation.

Blocked checks: none.

## Progress

- [x] Dependency and current tool/workspace seams characterized.
- [x] Failing tests added.
- [x] Bounded parser/tool implemented.
- [x] Focused and repository-wide gates passed.
- [x] Isolated released walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: use pure-Rust `pdf-extract` per-page output, not an external utility.
  Evidence: no `pdftotext` exists locally and managed external runtimes are deferred to EXT-088.
  Consequence: builds stay self-contained; malformed parser paths require an explicit panic boundary.
- Decision: reject encrypted PDFs rather than accept passwords.
  Evidence: credentials must not enter tool arguments, snapshots, or sessions.
  Consequence: users receive a fixed actionable unsupported-encryption error.
- Decision: abort extraction at the output cap instead of extracting and discarding the whole selected range.
  Evidence: bounded work is part of the feature, not just bounded display.
  Consequence: the marker reports unread remainder without inventing an omitted-byte count.

## Surprises and Discoveries

- Adding one Main-profile tool changed the prepared-request characterization count from 34 to 35 in bare mode; the full gate caught and the exact assertion was updated.
- Duplicate references in a PDF page tree are de-duplicated by the parser, so the 10,001-page cap fixture uses distinct page objects sharing bounded content.

## Recovery and Rollback

Remove the dependency, `ReadPdf` schema/handler, focused tests/snapshots, and documentation entries. There is no persisted migration or runtime data to recover.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/agent/{registry,tools}.rs`, `src/main.rs`, `src/tui/render_tests.rs`, four snapshots, harness documents, and this plan.

Commands/results: focused suites PASS; full 754 + 28 PASS; format, strict Clippy, release, smoke, snapshots, and released loopback walkthrough PASS.

Remaining risks: text extraction depends on PDF font encodings and intentionally provides no OCR/password support; parser panics are contained but the panic hook may still write a generic diagnostic to process stderr outside structured output capture.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `NEXT_STEPS.md`, `PARITY.md`, `TESTING.md`, and `docs/.INDEX.md` reconciled.
