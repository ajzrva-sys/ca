# EML-101 — Dedicated Draft-First Email Gateway

Status: COMPLETE
Owner/session: Codex `/root`
Last updated: 2026-08-13

## Goal

Add a dedicated, allowlisted IMAP gateway that processes only post-bootstrap inbound mail in fresh bounded no-tool requests and appends deterministic threaded replies to Drafts for human review, never SMTP-sending or auto-replying.

## Non-Goals

- No live mailbox, Keychain, provider, or real LaunchAgent validation without separate explicit opt-in.
- No attachment ingestion, allow-all/pairing mode, normal session/memory/hook/tool reuse, email-derived authority, or automatic send/reply.
- Do not reopen completed RND-097, SKL-098, SCH-099, or EML-100.

## Evidence and Current Behavior

- `docs/CAPABILITY_EXPANSION_HANDOFF.md#eml-101--dedicated-draft-first-email-gateway` is the frozen contract.
- EML-100 supplies bounded address/draft primitives and a separate reserved `ca.email` secret namespace, but no gateway runtime.
- `ScheduleDeliveryV2::EmailDraft` was previously inert; EML-101 now accepts only exact canonical account/recipient definitions and routes them to the Drafts-only host appender.
- `RuntimePaths::email_gateway_dir` supplies a separate profile root and the store adds exact account isolation beneath it.
- Primary dependency evidence: async-imap 0.11.3 exposes typed UID fetch/search and APPEND operations; mail-parser 0.11.5 exposes RFC 5322/MIME parsing primitives. Production TLS uses rustls with platform certificate verification.

## Scope

Allowed files are listed in the touched-file manifest below. Add a file before editing it.

Forbidden files/areas: `oracles/`, `reference/`, nested repositories, installed binaries, real Keychain/mailboxes/providers/LaunchAgents, and unrelated packet files.

Runtime state affected: none in user state. Tests use temporary roots, fake transports/model runners/launchctl, and one credentialless literal-loopback provider. The released offline fixture was moved recoverably to Trash after inspection.

## Design and Invariants

- Gateway configuration is disabled by default, dedicated-account-only, distinct from every EML-100 account ID, and has an exact normalized sender allowlist with no allow-all form.
- Credential bytes are resolved only from a profile/account-scoped `ca.email.gateway` Keychain item through an injectable store and never enter config, environment, argv, Debug, logs, events, or model output.
- Message identity is `(account_id, folder, UIDVALIDITY, UID)` plus bounded Message-ID/content digests. Bootstrap records the current high-water mark without changing message flags or processing older mail.
- MIME is bounded before and after decode; attachment bodies are never exposed. HTML is reduced to local text without remote loading.
- Each accepted message receives exactly one process-only provider request with a fixed system instruction, two messages, zero tools, and no session/memory/hook/checkpoint/portable state.
- Host composition addresses only the normalized allowlisted sender, produces a single `Re:`, bounded threading headers, deterministic Message-ID, plain UTF-8, and no attachment.
- Draft bytes and append recovery state are owner-only and hash-chained. Once APPEND starts, uncertainty is resolved only by exact Message-ID search; at most one exact-byte retry is allowed after a trustworthy absent result.
- CLI/service operations install nothing implicitly. Real install/uninstall require exact confirmation; tests use fake launchctl.
- Scheduled EmailDraft validates exact account/recipient values frozen inside the confirmed definition and invokes only the host Drafts writer, never SMTP.

## Milestones

### M1 — Types, config, secrets, and offline doctor
- Add bounded gateway configuration/types, dedicated-account validation, redacted secret boundary, disabled defaults, and safe offline doctor.

### M2 — MIME, transport, journal, and filtering
- Add strict MIME parsing, injectable IMAP abstraction, bootstrap/UIDVALIDITY journal, single-instance lock, sender/automation filters, and duplicate settlement.

### M3 — Isolated inference and Drafts recovery
- Add no-tool process-only model runner, deterministic composition, private payload recovery, APPEND ambiguity handling, and no-send proofs.

### M4 — CLI, daemon, LaunchAgent, and schedule
- Add `ca email` doctor/gateway/service surfaces, signal-aware polling, deterministic plist/fake launchctl lifecycle, and exact schedule EmailDraft activation.

### M5 — Completion
- Run fake transport crash matrix, focused and repository gates, update harness docs, and archive this plan.

## Acceptance Criteria

- Disabled/invalid/dedicated account and TLS configuration fail closed; empty allowlist produces zero model traffic.
- Bootstrap, UIDVALIDITY reset, duplicates, filters, MIME bounds, attachment omission, and cross-account isolation are deterministic under fakes.
- Accepted content enters one zero-tool provider request and no normal durable/observability surface.
- APPEND crash points cannot produce an automatic second draft under uncertain search state.
- No gateway production type has an SMTP-send method or path.
- CLI/plist/fake launchctl and schedule exact-recipient paths pass without real runtime mutation.
- Full tests, formatting, strict default/all-feature Clippy, release builds, released help/version, and snapshot hygiene pass.

## Validation

Focused: PASS — 29 release-profile email tests plus canonical schedule-delivery validation and request-snapshot privacy coverage.

Completion: PASS — 1,235 unit tests plus one accepted manual-browser ignore, 36 CLI tests, formatting, default/all-feature strict Clippy, and default/all-feature release builds.

Manual/visual: PASS — the released binary completed offline doctor, help/version/email help, deterministic `plutil`-linted service preview, and filesystem inspection under a disposable HOME.

Blocked checks: live dedicated mailbox, real Keychain auth, real provider request, and real LaunchAgent lifecycle are intentionally not authorized.

## Progress

- [x] Read repository instructions and frozen EML-101 contract.
- [x] Verify current transport/parser primary documentation and predecessor seams.
- [x] Complete M1.
- [x] Complete M2.
- [x] Complete M3.
- [x] Complete M4.
- [x] Complete M5.

## Decisions

- Decision: keep transport behind a narrow trait and validate with deterministic in-memory fakes.
  Evidence: the contract prohibits normal real-mailbox tests and requires crash-boundary proofs.
  Consequence: protocol safety can be exhaustively tested without credentials or traffic.
- Decision: use pinned async-imap 0.11.3, mail-parser 0.11.5, tokio-rustls 0.26.4, and rustls-platform-verifier 0.7.0.
  Evidence: their primary documentation exposes the required UID/APPEND, MIME, and platform-verification boundaries without a shell or mail-client subprocess.
  Consequence: production supports only certificate-verified implicit TLS or required STARTTLS and has no SMTP delivery method.

## Surprises and Discoveries

- The repository now has a usable root Git baseline, but prior packet work remains mixed in the working tree; the touched-file manifest remains the rollback authority.
- async-imap's APPEND API does not surface APPENDUID. The adapter therefore searches the deterministic Message-ID after APPEND and treats absent/multiple/unavailable results conservatively.

## Recovery and Rollback

Inspect or manually revert only files/hunks in the manifest. Do not use broad Git reset/clean because unrelated completed packet work remains uncommitted.

## Touched-File Manifest

- `docs/exec-plans/completed/eml-101-dedicated-draft-first-email-gateway.md`
- `Cargo.toml`
- `Cargo.lock`
- `src/config.rs`
- `src/runtime.rs`
- `src/request.rs`
- `src/main.rs`
- `src/email/mod.rs`
- `src/email/secrets.rs`
- `src/email/store.rs`
- `src/email/gateway.rs`
- `src/email/imap.rs`
- `src/email/mime.rs`
- `src/email/launchd.rs`
- `src/schedule.rs`
- `src/schedule_v2.rs`
- `NEXT_STEPS.md`
- `ARCHITECTURE.md`
- `CONTINUITY.md`
- `ENVIRONMENT.md`
- `TESTING.md`
- `docs/.INDEX.md`

## Completion Record

Changed files: the touched-file manifest above. Gateway implementation is contained in the five new `src/email/` modules; existing config/runtime/request/main/schedule files received narrow integration changes.

Commands and results:

- `cargo test --no-fail-fast` — PASS: 1,235 unit tests, one accepted manual browser test ignored, and 36 CLI tests.
- `cargo test --release email:: --no-fail-fast` — PASS: 29 tests.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --all-targets -- -D warnings` — PASS.
- `cargo clippy --all-features --all-targets -- -D warnings` — PASS.
- `cargo build --release` — PASS.
- `cargo build --release --all-features` — PASS.
- released `--version`, `--help`, and `email --help` — PASS.
- released disposable `email doctor` and `email service preview` — PASS offline; preview passed `/usr/bin/plutil -lint` internally and created no runtime data.
- `git diff --check` and snapshot hygiene — PASS.

Remaining risks: interoperability with a real dedicated mailbox, real Keychain item, server-specific Drafts mapping/APPENDUID behavior, and real provider remains unverified by design. `email doctor --connect` and real service lifecycle require separate explicit authorization.

Harness updates: `NEXT_STEPS.md`, `ARCHITECTURE.md`, `CONTINUITY.md`, `ENVIRONMENT.md`, `TESTING.md`, and `docs/.INDEX.md` record the completed boundary and evidence. The capability-expansion sequence is complete; no successor packet is declared.
