# EML-100 — Typed Himalaya Mailbox Tools

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-13

## Goal

Expose exactly six opt-in typed mailbox tools over a trusted Himalaya v2.0.0 executable, with bounded private data, immutable ca-owned drafts, exact one-use outbound approval, and delivery-uncertainty semantics that never retry.

## Non-Goals

- No arbitrary shell, editor, mailbox mutation, rules, automatic sending, credential import, or AgentMail integration.
- No live mailbox, provider, credential, or user-runtime validation.
- Do not reopen completed RND-097, SKL-098, or SCH-099.

## Evidence and Current Behavior

- `docs/CAPABILITY_EXPANSION_HANDOFF.md#eml-100--typed-himalaya-mailbox-tools` is the frozen contract.
- `src/agent/registry.rs` has no mailbox capabilities or tools.
- `src/tool_selection.rs` has explicit opt-in handling for browser but no `email_enabled` constraint.
- `src/permission.rs::derive_session_grant` falls back to a whole-tool grant for tools without paths; that is forbidden for outbound email.
- `src/main.rs::execute_tool_dispatch` currently puts raw tool input and output into hook payloads.
- Headless Yolo currently allows prompted tools; it cannot authorize external communication.
- Primary upstream evidence: Himalaya v2.0.0 is the current release (2026-07-26). Its source defines global `--json`, `-a/--account`, `-c/--config`, `envelope list`, `message read`, and `message send` with piped RFC 5322 input. v2 removed `HIMALAYA_CONFIG`; explicit `--config` remains. The release also documents machine-readable JSON schemas and the shared command API.

## Scope

Allowed files are listed in the touched-file manifest below. Add a file there before editing it.

Forbidden files/areas: `oracles/`, `reference/`, nested repositories, installed binaries, user config/mailboxes/credentials, and real LaunchAgent state.

Runtime state affected: none during implementation. Tests use temporary owner-only roots and fake executables only.

## Design and Invariants

- Tools: `email_accounts`, `email_list`, `email_read`, `email_draft`, `email_send`, `email_reply`; no others.
- Email tools are absent unless the configured email subsystem is enabled and the exact tool is explicitly selected. Bare, plan, review, subagent, scheduled, and gateway contexts cannot gain them.
- Message identity is `(account_id, folder, message_id)`.
- The adapter supports exactly Himalaya v2.0.x, uses a revalidated absolute executable, direct argv, cleared environment, piped stdin, bounded output, timeout/process-group termination, typed JSON, and suppressed raw process errors.
- Drafts and staged attachments are immutable, owner-only, digest-bound records. Any drift fails closed.
- Outbound permission is a process-local single-use approval bound to account, complete canonical recipients, draft and attachment digests, tool call, session, and expiry. Yolo, AutoReview, generic rules, network grants, and persistent/session tool grants cannot authorize it.
- Once send stdin starts, every non-success outcome is `DeliveryUnknown`; there is no automatic retry.
- Email calls use metadata-only hooks/events. Durable and portable tool arguments/results use protocol-valid private markers; raw body/address data exists only in the live request/tool loop.

## Milestones

### M1 — Characterization and public boundary

- Freeze disabled selection, generic approval rejection, metadata-only payloads, and current Himalaya argv in focused tests.
- Add capabilities, bounded types, disabled config, runtime paths, and redacted Debug.

### M2 — Store, approval, and adapter

- Add immutable draft/attachment storage and delivery journal.
- Add exact one-use approvals and v2.0 adapter with fake-executable tests.

### M3 — Typed operations

- Add account/list/read, draft, send, and reply manager operations.
- Prove exact argv/stdin, bounds, reply threading, ambiguity, and no retry.

### M4 — Host integration and privacy

- Inject the manager into interactive, headless, and service execution.
- Add typed outbound preview, unsafe approval-key denial, and independent persistence/observability scrubbing.

### M5 — Completion

- Run focused tests and all required repository gates.
- Perform a released fake-Himalaya walkthrough, update harness docs, and archive this plan.

## Acceptance Criteria

- Exactly six schemas exist and are absent by default in every profile.
- All inputs/results, folders/IDs, addresses, bodies, attachments, subprocess bytes, and time are bounded.
- Exact fake argv/stdin/environment tests cover all Himalaya operations.
- Generic permissions, Yolo, AutoReview, stale/mutated/duplicate approvals cannot send.
- Post-start failure is `DeliveryUnknown` and executes only once.
- Recursive sentinels do not appear in hooks, routine events, sessions, checkpoints, portable exports, or request snapshots.
- Full tests, formatting, strict default/all-feature Clippy, and release builds pass.

## Validation

Focused: PASS — 16 release-profile email tests plus dedicated private-sentinel, Yolo/generic-grant, service, registry, and wide/narrow render coverage.

Completion: PASS — full default suite, formatting, default/all-feature strict Clippy, and default/all-feature release builds.

Manual/visual: PASS — optimized fake-Himalaya suite exercised exact argv/stdin/environment, v2 JSON, immutable drafts, exact approval, Bcc omission, and ambiguous delivery without live mailbox access.

Blocked checks: live mailbox intentionally not authorized or required.

## Progress

- [x] Read repository instructions and frozen EML-100 contract.
- [x] Verify current Himalaya release and primary v2.0.0 CLI source.
- [x] Complete M1.
- [x] Complete M2.
- [x] Complete M3.
- [x] Complete M4.
- [x] Complete M5.

## Decisions

- Decision: target only Himalaya `2.0.x`.
  Evidence: v2.0.0 is the current release and its command/output contract breaks from v1.2.0.
  Consequence: version mismatch fails before mailbox access.
- Decision: send raw deterministic RFC 5322 through stdin to the SMTP-specific `smtp send` command with explicit envelope recipients.
  Evidence: v2.0.0 accepts piped RFC 5322 input for `smtp send`; explicit `--mail-from` and repeated `--rcpt-to` keep Bcc recipients envelope-only.
  Consequence: ca owns composition, never exposes a Bcc header, and never asks Himalaya to read mutable draft/attachment paths.

## Surprises and Discoveries

- Himalaya v2.0.0 was released after the handoff was written. It replaces several v1 command names, drops `HIMALAYA_CONFIG`, and makes JSON schemas an explicit CLI feature.

## Recovery and Rollback

Use the manifest below to inspect or manually revert only EML-100 hunks. Never use broad Git reset/clean because the repository has no baseline commit and contains other packets' untracked work.

## Touched-File Manifest

- `docs/exec-plans/completed/eml-100-typed-himalaya-mailbox-tools.md`
- `NEXT_STEPS.md`
- `ARCHITECTURE.md`
- `CONTINUITY.md`
- `ENVIRONMENT.md`
- `TESTING.md`
- `docs/.INDEX.md`
- `src/email/mod.rs`
- `src/email/types.rs`
- `src/email/himalaya.rs`
- `src/email/store.rs`
- `src/email/secrets.rs`
- `src/config.rs`
- `src/runtime.rs`
- `src/agent/registry.rs`
- `src/agent/tools.rs`
- `src/tool_selection.rs`
- `src/permission.rs`
- `src/app.rs`
- `src/approval.rs`
- `src/interaction.rs`
- `src/main.rs`
- `src/headless.rs`
- `src/service.rs`
- `src/hooks.rs`
- `src/session.rs`
- `src/session_portability.rs`
- `src/context.rs`
- `src/request.rs`
- `src/tui/render.rs`
- `src/tui/render_tests.rs`
- `src/tui/snapshots/ca__tui__render_tests__outbound_email_approval_wide.snap`
- `src/tui/snapshots/ca__tui__render_tests__outbound_email_approval_narrow.snap`

## Completion Record

Changed files: the touched-file manifest above. The new implementation is contained in `src/email/`; existing host/config/privacy modules received narrow integration changes and two deterministic approval snapshots.

Commands and results:

- `cargo test --no-fail-fast` — PASS: 1,219 unit tests, one accepted manual browser test ignored, and 36 CLI tests.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --all-targets -- -D warnings` — PASS.
- `cargo clippy --all-features --all-targets -- -D warnings` — PASS.
- `cargo build --release` — PASS.
- `cargo build --release --all-features` — PASS.
- `cargo test --release email:: --no-fail-fast` — PASS: 16 fake-Himalaya/type/store/approval/privacy tests.

Remaining risks: production mailbox interoperability still depends on an explicitly configured trusted Himalaya v2.0.x installation. No live mailbox is required or authorized by EML-100. EML-101 remains separate and draft-only.

Harness updates: `NEXT_STEPS.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, and `docs/.INDEX.md` now record the boundary and completion evidence.
