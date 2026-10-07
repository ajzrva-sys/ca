# BRW-086 — Browser Automation

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12
Archive: completed after all focused, repository-wide, release, and disposable-runtime gates passed.

## Goal

Provide an explicitly enabled `browser` tool that drives an existing local Chrome/Chromium through a disposable literal-loopback CDP session, with separate approval for every tool call and every network-bearing navigation/download.

## Non-Goals

- Installing or updating a browser.
- Reusing a user's browser profile, cookies, extensions, sessions, or credentials.
- Persisting browser state, granting background network access, or supporting remote CDP.
- Adding a TUI browser or claiming Cursor parity.

## Evidence and Current Behavior

- `src/tool_selection.rs` currently makes every Main-profile built-in eligible by default; opt-in tools therefore need an explicit-selection gate in the central resolver.
- `src/workspace.rs::prepare_tool_invocation` already separates network prompts from ordinary tool/path approvals and can classify network per invocation.
- `src/agent/tools.rs::execute_tool_with_context` is the typed built-in execution seam. `src/image_content.rs` already normalizes bounded PNGs without retaining source metadata.
- Process-group ownership/cleanup patterns exist in `src/jobs.rs`, `src/hooks.rs`, and `src/lsp.rs`.
- A local Chromium executable currently exists at `/Applications/Chromium.app/Contents/MacOS/Chromium`; validation must still use injected fake executables/loopback fixtures before any optional disposable real-browser walkthrough.
- Ante documentation describes Browser as an optional approval-gated CDP tool (`oracles/ante-main/docs-site/docs/reference/tools-reference.mdx:70-73`, `architecture.mdx:126`, and `configuration/permission.mdx:82`). This is descriptive oracle evidence only.

## Scope

Allowed files: `src/browser.rs`, `src/main.rs`, `src/agent/{registry,tools}.rs`, `src/{workspace,tool_selection,image_content}.rs`, focused tests, this plan, and the five harness documents.

Forbidden: `oracles/`, `reference/`, real profiles/cookies/downloads, user config/Keychain/sessions, installed binaries.

Runtime state: one process-owned ephemeral browser at a time; disposable profile removed on close/shutdown.

## Design and Invariants

- Register one typed `browser` tool with `start`, `navigate`, `snapshot`, `click`, `type`, `screenshot`, `download`, and `close` actions.
- The tool is absent unless named by `--tools` or `--include-tools`; exclusion still wins. Unknown names continue failing before provider traffic.
- Keep the tool mutating so existing approval policy prompts on every invocation. Only `navigate` and `download` set `network_requested`, preserving the distinct one-run network approval.
- Discover existing executables conservatively, or use an injected executable in tests. Launch headless with a new private temporary user-data directory, loopback port zero, background services disabled, cleared ambient environment, and an owned process group.
- Accept only a bounded `DevToolsActivePort`, literal-loopback page WebSocket, bounded CDP frames/responses, and a ten-minute process lifetime.
- Block HTTP(S)/WS(S) at CDP outside an approved navigation/download. Navigation accepts only body-free HTTP(S) URLs without URL credentials and never returns the URL in errors/output.
- Bound sanitized DOM output to 64 KiB. Normalize screenshots, cap source/output, and atomically create one approved workspace file. Downloads are capped at 16 MiB, written atomically without overwrite, and never expose server-selected filenames.
- Bind the active browser to the originating `JobOwner`; another run cannot inspect or close it. Global shutdown always kills/reaps the process group and removes the profile.
- No endpoints, page URLs, cookies, headers, response bodies, screenshot bytes, or download bytes enter debug output, sessions, snapshots, or manager state.

## Milestones

### M1 — Characterization and selection boundary

- Add failing registry/selection/workspace tests for typed actions, default omission, explicit inclusion, exclusion precedence, path preparation, and per-action network requests.
- Validation: focused registry/tool-selection/workspace tests.

### M2 — Ephemeral CDP manager

- Add executable discovery, private launch, strict loopback target discovery, bounded protocol requests, owner/lifetime enforcement, default network blocking, and cleanup.
- Add fake-executable/loopback tests for launch, wrong/non-loopback endpoints, timeouts, malformed/oversized responses, ownership, and descendant cleanup.

### M3 — Bounded browser actions

- Implement navigation, sanitized DOM, click/type, normalized screenshot persistence, bounded download persistence, and close.
- Test approval inputs, blocked unapproved network, selectors/text escaping, DOM bounds, screenshot bounds/normalization, download caps/no-overwrite, timeout, and post-close state.

### M4 — Runtime and harness completion

- Register process shutdown, update architecture/continuity/testing/next-steps/parity, run full gates, and perform a disposable-home fake-CDP walkthrough. A real local Chromium walkthrough is optional and may touch only a temporary profile/workspace.

## Acceptance Criteria

- Default requests never expose `browser`; explicit selection does.
- A browser starts only from an existing executable and never uses a persistent profile.
- Navigation/download cannot execute without distinct network authority; non-network actions execute with external network blocked.
- Every observable payload and file result is bounded, sanitized, and secret-free.
- Cross-run access, unsafe endpoints/URLs/paths, overwrite, stale lifetime, malformed CDP, and process leaks fail closed.
- Shutdown removes the temporary profile and all owned descendants.

## Validation

Focused: PASS — `cargo test --bin ca browser --no-fail-fast` plus registry/selection/workspace tests.

Completion: PASS — 1032 unit tests plus 33 CLI integration tests, format, strict Clippy, release build, and help/version/explicit-selection smokes.

Manual/visual: PASS — the explicitly invoked ignored test drove the existing local Chromium against a loopback page through a disposable profile, read bounded DOM, saved a normalized PNG, closed, and leaked no process/profile. No TUI state was added.

Blocked checks: none.

## Progress

- [x] Source, approval, multimodal, process-cleanup, and oracle seams inspected.
- [x] Characterization tests and central opt-in boundary.
- [x] CDP manager and bounded actions.
- [x] Full gates and disposable walkthrough.
- [x] Harness reconciliation and plan archival.

## Decisions

- Decision: one action-oriented tool rather than several tools.
  Evidence: the authority boundary is one approval-gated process while network varies per action.
  Consequence: tool selection remains simple and action validation is closed.
- Decision: screenshots are normalized into an approved workspace path; image-capable models may inspect them separately with `view_image`.
  Evidence: this avoids silently widening model modality requirements and keeps screenshot bytes out of protocol state.
  Consequence: DOM automation remains usable on text-only models.
- Decision: a browser instance is owner-bound and process-owned.
  Evidence: global mutable browser state would otherwise cross service/subagent/main-run boundaries.
  Consequence: concurrent owners fail closed instead of sharing cookies/page state.

## Surprises and Discoveries

- `DevToolsActivePort` can be observed during a partial write; startup now retries incomplete bounded content until the launch deadline rather than treating a transient prefix as a final protocol failure.
- Cancellation of a caller future would otherwise leave a temporarily unblocked page. Browser operations run in a bounded owned task, so the network re-block or exact process teardown completes even when the waiting tool call is cancelled.

## Recovery and Rollback

Stop the manager, kill its exact process group, remove only its `TempDir`, and revert the local BRW-086 files with a targeted patch. Do not use destructive Git reset commands.

## Completion Record

Changed files: `Cargo.toml`, `Cargo.lock`, `src/browser.rs`, `src/main.rs`, `src/agent/{registry,tools}.rs`, `src/{workspace,tool_selection,image_content}.rs`, `src/service.rs`, `src/agent/subagent.rs`, and the harness/plan documents.

Commands and results: focused browser PASS; full repository PASS (1032 unit, 33 CLI, one manual ignored); format PASS; strict Clippy PASS; release PASS; local-Chromium and release smoke PASS.

Remaining risks: CDP is versioned by the installed browser rather than pinned by `ca`; unsupported local browser versions fail closed. Browser automation is deliberately headless, one-owner, one-page, and opt-in.

Harness updates: `.INDEX.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `PARITY.md`, `TESTING.md`, and `NEXT_STEPS.md` reconciled.
