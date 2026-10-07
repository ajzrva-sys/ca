# MM-067 — Typed Multimodal Content and Image Inspection

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-11

## Goal

Replace raw JSON/base64 image handling with typed text/image request blocks and add one bounded read-only `view_image` tool that safely decodes, normalizes, and exposes local workspace images only to image-capable provider adapters, while persisted sessions retain unavailable-safe metadata and never image bytes.

## Non-Goals

- OCR, image generation/editing changes, video/audio, remote image downloads, SVG execution, animation playback, original metadata preservation, or a terminal image renderer.
- Adding image support to models explicitly configured text-only.
- Persisting, caching, or restoring image bytes across processes.
- Cursor parity. This is an oracle-informed `ca` extension.

## Evidence and Current Behavior

- `ChatMessage.content` is arbitrary `serde_json::Value`; `AgentClient::build_messages` inserts clipboard base64 into a Chat-Completions-specific JSON shape before provider adapters (`src/agent/client.rs`). Responses and Anthropic explicitly reject non-text values.
- `AppState::pasted_images` holds untyped base64 strings, clipboard input encodes PNG without pre-decode byte/pixel bounds, and images are cleared after request construction (`src/app.rs`, `src/tui/input.rs`, `src/main.rs`).
- `ModelProfile.supports_images` exists, but it is a nullable boolean and has no typed modality set or adapter validation (`src/config.rs`).
- `/request` already replaces data-image payloads with binary metadata; Session V4 currently persists no image marker at all (`src/request.rs`, `src/session.rs`).
- `image = 0.25` is already available. The central resource-preparation boundary can authorize a new read-only local-path tool without adding filesystem authority (`src/agent/registry.rs`, `src/workspace.rs`).

## Scope

Allowed files: typed request/provider/client/config/app/session boundaries; central registry/tools/workspace; clipboard input and orchestration; request/context accounting; focused generated-image tests and snapshots; this plan and required harness documents.

Forbidden: oracle/reference trees, user runtime state, remote images, raw image bytes in Debug/snapshots/sessions, external decoder executables, generated-image behavior changes, unrelated provider protocol changes, and installed-binary mutation.

Runtime state affected: bounded process-only normalized image bytes and unavailable-safe attachment markers. Tests use generated images and fake providers inside temporary workspaces/homes.

## Design and Invariants

- Introduce a closed `ModelContentBlock` type above adapters: UTF-8 text or normalized image bytes plus an allowlisted MIME type and safe dimensions. `ChatMessage` owns typed blocks rather than arbitrary JSON content; tool-call/result message identity stays unchanged.
- Adapter-owned serialization maps the same typed blocks to Chat Completions `image_url`, Responses `input_image`, and Anthropic base64 `image` source shapes. Adapters reject unsupported roles/content before HTTP construction. Text-only requests preserve their existing exact bytes.
- Replace nullable image support with a default-compatible typed modality set while continuing to read legacy `supports_images`. Missing modality remains text-only unless the existing exact model profile explicitly opted into images; no model-name inference.
- Add process state containing normalized PNG bytes, width/height, and media type. Custom Debug omits bytes and byte counts. Raw clipboard buffers are bounded before allocation where possible, decoded dimensions are checked before re-encoding, and EXIF/ICC/text/other metadata are stripped by decoding to pixels and re-encoding a still PNG.
- Initial limits: at most four pending images, 10 MiB compressed input each, 40 million decoded pixels each, 64 MiB decoded RGBA each, and 20 MiB aggregate normalized bytes. Animated formats contribute only a safely decoded first frame; SVG is rejected.
- `view_image { path }` is read-only and available in Main/Subagent/Review only when the exact selected model profile includes image input. It accepts workspace regular PNG/JPEG/WebP/GIF/BMP/TIFF files, applies the same limits/normalizer on a blocking worker, and returns a typed image tool result. It does not accept URLs or emit base64 into model-visible text.
- Extend tool execution with a typed output variant for image content. TUI/headless text/JSON/sessions receive only a safe marker such as `[image/png 640×480; image available only in this process]`; only the immediate prepared provider continuation may receive bytes. Serialized actions/events, approvals, hooks, usage, request snapshots, and Debug output never expose the payload; transient process-only input and stream handoffs own redacted typed values.
- Before canonical append, a submission with pending images captures exact process-only typed blocks for that turn and adds only safe attachment markers to the persisted user message. The first prepared request receives the bytes; retry/resume after those bytes are gone retains the marker and clearly cannot reconstruct the image.
- Hook denial and incompatible pre-I/O selection preserve pending images for edit/retry in the current process. Successful request handoff moves bytes into the generation and clears the composer state; later provider failure or cancellation cannot reconstruct them. Generation IDs prevent stale clipboard results from entering a later request.
- Context accounting uses normalized compressed bytes for process-live attachments and marker text after persistence. `/request` renders only MIME/dimensions and a payload-omitted marker.

## Milestones

### M1 — Typed request characterization

- Add failing tests for typed text/image blocks, exact unchanged text-only bytes, per-adapter image serialization, unsupported-role/modality rejection, and safe request snapshot/debug behavior.
- Add config compatibility tests for legacy `supports_images` and explicit modality values.

### M2 — Safe normalization and clipboard state

- Add the shared blocking image normalizer and typed process-only `PendingImage` state.
- Test compressed/pixel/decoded/aggregate/count caps, corrupt and unsupported input, metadata stripping, safe re-encoding, debug redaction, hook denial, cancellation, and incompatible-model preservation.

### M3 — Typed `view_image` execution

- Add registry schema/resource/profile metadata and typed tool output/continuation wiring.
- Test workspace traversal/symlink rejection, regular-file/extension/format mismatch, generated PNG/JPEG/WebP/GIF fixtures, modality gating, headless/TUI safe markers, payload process lifetime, and stale-generation cancellation.

### M4 — Persistence, render, and gates

- Persist only safe unavailable markers, keep existing sessions readable, and prove retry/resume cannot synthesize image bytes.
- Add reviewed wide/narrow clipboard, tool success, incompatibility, and safe failure snapshots.
- Run focused/full/format/strict-Clippy/release/smoke gates plus a released temporary-home loopback walkthrough for clipboard-equivalent and `view_image` continuations.

## Acceptance Criteria

- Existing text-only provider requests remain byte-for-byte unchanged.
- All three provider adapters can serialize one normalized image through their documented typed request shape, and configured text-only models fail before network I/O.
- Oversized, corrupt, decompression-bomb-like, external-workspace, SVG, or unsupported images fail without retaining payload/path/parser internals.
- Metadata is absent after normalization, dimensions/MIME are truthful, and no raw/base64 image appears in Debug, TUI/headless text, structured events, snapshots, hooks, config, or Session V4.
- A successful `view_image` call can supply typed image content to exactly the next provider continuation without widening filesystem, approval, sandbox, or network authority.
- Saved/restored history contains only an unavailable-safe marker; pending images survive local pre-I/O failures but not successful request handoff or process exit.

## Validation

Focused: PASS. Typed-content/provider/config/request suites, image normalizer/tool/session tests, and headless exact-continuation coverage passed.

Completion: PASS. `cargo test --no-fail-fast` passed 768 unit tests plus 28 CLI integration tests; `cargo fmt --check`, strict Clippy, release build, and help/version/named-profile smoke tests passed.

Manual/visual: PASS. Six wide/narrow snapshots were reviewed. A released-binary temporary-home loopback run made exactly two requests: the first called `view_image`, the second contained one image block plus its safe marker, completed successfully, and emitted no base64 in JSONL.

Blocked checks: none.

## Progress

- [x] Current raw image/request/config/session seams characterized.
- [x] Failing typed-content and normalization tests added.
- [x] Provider adapters and process-only image state implemented.
- [x] `view_image` typed tool path implemented.
- [x] Persistence/render coverage and repository-wide gates passed.
- [x] Isolated released walkthrough passed.
- [x] Harness reconciled and plan archived.

## Decisions

- Decision: type content above provider adapters instead of teaching adapters to recognize arbitrary JSON.
  Evidence: current Chat Completions JSON is assembled before protocol selection and the other adapters deliberately reject it.
  Consequence: text-only compatibility needs exact characterization before structural edits.
- Decision: normalize every accepted image to still PNG in memory.
  Evidence: pixel decode/re-encode strips metadata and gives one bounded format for adapter serialization.
  Consequence: animation and original encoding/metadata are intentionally not preserved.
- Decision: persist only safe markers, never recoverable paths or bytes.
  Evidence: sessions may outlive workspace authority and raw image payloads can contain sensitive data.
  Consequence: resumed/retried historical turns cannot resend unavailable images and must say so explicitly.

## Surprises and Discoveries

- Clipboard image support already exists but bypasses compressed-byte and decoded-pixel limits and is coupled directly to Chat Completions JSON.
- `supports_images` is present but its missing-value behavior is not a safe capability declaration for a multi-adapter typed content boundary.

## Recovery and Rollback

Restore `ChatMessage` text representation, remove the typed image/result variants and `view_image`, retain session compatibility readers for any safe markers already written, and remove only the new tests/snapshots/docs. No persisted image bytes require migration or deletion.

## Completion Record

Changed files: typed image/content state in `src/image_content.rs`; provider/client/config/request boundaries; tool registry/execution; main/headless/review/subagent continuation orchestration; TUI input/render/state; focused tests/snapshots; harness documentation.

Commands and results: focused multimodal suites PASS; `cargo test --no-fail-fast` PASS (768 unit, 28 CLI); `cargo fmt --check` PASS; `cargo clippy --all-targets -- -D warnings` PASS; `cargo build --release` PASS; help/version/named-profile smoke PASS.

Remaining risks: historical unavailable markers intentionally cannot recreate bytes after handoff/process exit; animated inputs use one decoded still frame; provider image behavior is fixture-proven rather than live-provider tested.

Harness updates: `ARCHITECTURE.md`, `CONTINUITY.md`, `TESTING.md`, `NEXT_STEPS.md`, `PARITY.md`, `ENVIRONMENT.md`, and `docs/.INDEX.md` reconciled. Oracle/reference trees remained read-only.
