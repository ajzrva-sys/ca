# BUG-239 — Restore the native Codex model catalog

Status: COMPLETE (installation canceled by user; unrelated protected-tree gate separately blocked)
Owner/session: Codex, current user-requested implementation
Last updated: 2026-09-03

## Goal

CA's exact first-party `openai-codex` route displays the account's authenticated
native catalog, including visible Codex-only models, rather than a larger
hardcoded fallback. Verify catalog-only live discovery and build the release.
The user's latest instruction is **do not install this time**.

## Non-Goals

No capability/pricing profiles, config migrations, inference requests, general
OpenAI API catalog changes, or oracle-baseline repair are part of this request.

## Evidence and Current Behavior

- `src/model_catalog.rs` currently requests generic `/models`, uses only bearer
  auth, and decodes `data[].id`; the user's picker reports HTTP 400.
- Read-only protocol observations: Codex `endpoint/models.rs` requests
  `client_version`; `protocol/src/openai_models.rs` defines `models`, `slug`,
  and `visibility: list`. Returned instruction text is not CA configuration.
- The non-secret local cache observed during planning uses compatibility
  version `0.153.0` and has seven visible entries, including Spark with
  `supported_in_api: false`. This is fixture evidence, not runtime authority.
- Official account/access context:
  https://learn.chatgpt.com/docs/enterprise/workspace-model-availability
- The user approved `0.153.0` compatibility while identifying CA honestly.

## Scope

Allowed: catalog request/decoder and tests; app in-memory catalog state; both
refresh handlers; picker/provider rendering and snapshots; tracking documents.
Oracle/reference trees stay read-only. No credential values may be output.
Runtime: one catalog-only check via CA's existing auth resolver; no user config
or sessions changed. The user explicitly overrides the standing install rule
for this task: do not replace the installed executable.

## Design and Invariants

- Exact route classification isolates native Codex from proxies/custom URLs.
- Minimal native decoder ignores all metadata except slug and list visibility.
- Preserve 10s timeout, 1 MiB streamed response bound, ID/count limits, sorting,
  deduplication, and sanitized errors.
- Successful native catalogs replace discovered IDs, without documented merge.
  A separate in-memory last-success map distinguishes actual catalogs from
  documented fallbacks (including successful empty catalogs).
- Failed native refresh retains last-success IDs and visible failure status;
  without prior success use existing fallback. Configured/current IDs remain
  selectable but missing-from-catalog entries are labeled configured only.
- Provider changes invalidate the associated last-success state. No persistence.

## Milestones

1. Failure-first native-format characterization; implement request/decoder and
   loopback tests for headers, visibility, bounds, failures and route isolation.
2. Shared native state application for both refresh paths; selection, recovery,
   configured-only and wide/narrow render coverage.
3. Explicit ignored live test invokes CA authentication and discovery without
   inference, compares returned IDs with picker state, reports only safe IDs.
4. Full completion checks, docs and release artifact checks. Installation was
   explicitly canceled by the user before any installer was launched.

## Acceptance Criteria

Native fixture yields exactly seven visible IDs including non-API Spark and
models beyond GPT-5.6. Both refresh paths preserve current/selected entries,
never merge fallback into native success, handle empty success and repeated
failure/recovery truthfully. Real authenticated discovery succeeds and picker
entries match it. Tests, formatting, strict Clippy and release artifact checks
pass; the installed executable stays unchanged. The unrelated protected-tree
discrepancy stays explicit.

## Validation

Focused: catalog unit tests, main refresh tests, picker snapshots/input tests.
Completion: `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`,
`cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo build --release --workspace --all-features`, release help/version.
Manual/visual: review paired snapshots and render real returned catalog.
Blocked checks: existing oracle committed-baseline discrepancy, separate task.

## Progress

- [x] Inspect repository, approved plan and native protocol evidence.
- [x] Record failing-first test and implement catalog protocol.
- [x] Implement refresh/selection/status and deterministic UI coverage.
- [x] Validate real authenticated catalog-only refresh and matching picker.
- [x] Run completion checks and record results.
- [x] Honor the user's per-task no-install override; no installer launched.

## Decisions

Keep only minimal native catalog fields; never use remote instructions. Keep
the generic catalog path untouched for custom endpoints and other providers.

The user's final delivery instruction cancels installation for this task only;
the standing rule in AGENTS.md is unchanged. A temporary installer was prepared
but never launched, and was discarded after the cancellation. Release candidate SHA-256:
`1ab5d08e0b5075f353828c613e24deeee2cc4849627c7109657f9b1ca5f292a4`.

## Surprises and Discoveries

The failure-first native fixture test returned `invalid model catalog` with
the generic decoder. Native request/response tests now pass. A selection test's
initial expectation was corrected: bare mode intentionally adds a chosen model
to in-memory provider models, while leaving persisted config untouched; that
existing behavior is preserved.

The explicit live test succeeded in 0.74s using CA authentication and the native
request. It returned exactly seven visible models (Spark, 5.4, 5.4-mini, 5.5,
5.6-luna, 5.6-sol, 5.6-terra). All matched picker IDs and rendered across the
real wide/narrow pages; only IDs and safe render output were logged at
`/tmp/ca-bug239-live.log`. No inference was requested. The previous fallback
repair did not repair discovery; this live result validates the new path.

## Recovery and Rollback

Preserve unrelated dirty-tree changes. Use local reverse patches if needed,
never Git reset. No installed executable is replaced, so installation rollback
is unnecessary. Keep the existing executable unchanged; retain the built
candidate separately under `target/release/ca`.

## Completion Record

Changed files:
- `src/model_catalog.rs`: exact native request, CA/account headers, minimal
  decoder and loopback regressions.
- `src/app.rs`, `src/main.rs`: last-success state, shared native refresh logic,
  configured/current union, selection restoration and refresh regressions.
- `src/tui/input.rs`, `src/tui/render.rs`, `src/tui/render_tests.rs`: reuse actual
  grouped picker selection, configured-only labels, fixture/real-response renders.
- `tests/fixtures/codex-model-catalog.json`: synthetic seven-visible native fixture.
- Five `src/tui/snapshots/*native_codex*.snap` files: recent/middle/configured,
  cached failure and provider-specific catalog views at both widths.
- `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`, `ARCHITECTURE.md`, `PARITY.md`,
  `docs/.INDEX.md` and this plan: design, evidence, validation and no-install override.

Passing checks: failure-first reproduction then 11 focused native tests,
73 broader catalog tests, final all-feature catalog suite 18/18, five paired
snapshots, authenticated live catalog/picker comparison, formatting, strict
all-target/all-feature workspace Clippy, release build/signature/version/help,
diff/snapshot hygiene. All-feature CA unit suite: 2,473 passed/5 ignored.
The full all-feature workspace command completed with exit 0: CA 2,473/5
ignored, experiment 118/1 ignored, CLI 38, role-run 17, codex-team 652/1
ignored, MCP stdio 8, and all remaining integration/protocol/doc-test targets
passed. No failures. The final all-feature focused catalog run also passed
after the test-helper Clippy correction.

Logs: `/tmp/ca-bug239-{focused,catalog,final-catalog,live,workspace,clippy,fmt,release,protected}.log`.
The macOS oversized `__eh_frame` test-link warning is pre-existing and nonfatal.
The protected-tree gate still reports the pre-existing oracle committed-baseline
mismatch; direct oracle/reference Git status is empty. No receipt/tree repair.

Delivery: release built, not installed at the user's final request. Candidate
SHA-256 is recorded above; installed SHA-256 remains
`0336bd2050b525d71959f885af40ccc603873f534a0821b85e8bdd3c53fd0192`.
No inference, capability/profile migration, or user config/session writes.
