# BUG-268 — Current Sol catalog and usable provider refresh

Status: COMPLETE and installed
Date: 2026-10-09

## Goal and observations

The user asks to show GPT-6.1 Sol and make provider selection/refresh work.
CA's native catalog requests compatibility version 0.153.0, while the local
non-secret Codex model cache uses 0.162.1 and lists `gpt-6.1-sol`. Existing
fallbacks omit that model. Provider-list refresh has no handler; initial global
discovery is skipped after or during a scoped refresh. Global refresh also fails
to restore the provider detail's selected model identity. Crossterm infers Shift
for uppercase R; the original modifier-only diagnosis was incorrect. The repair
also accepts uppercase R with no modifier for direct/legacy event compatibility.

## Scope and invariants

Update native catalog compatibility and the exact official-route fallback;
add Sol's exact capability profile; repair provider/picker key dispatch and
selection preservation. Live catalogs remain authoritative, failures preserve
available cached/configured choices, and hidden models stay hidden. Preserve
all pre-existing edits. No user config/session mutation or live inference requests.
Validation uses disposable state and loopback providers; a separate existing
catalog-only authenticated test may verify actual returned identifiers through
CA's normal authentication boundary, without exposing credentials.

## Work and acceptance

1. Preserve before-images and demonstrate failing focused/actual terminal paths.
2. Add narrow catalog/profile and selection/refresh regressions; fix behavior.
3. Real terminal cases must select the intended provider/model, refresh using R,
   show new models, retain correct row identity, and recover from catalog failure.
4. Run focused tests, deterministic rendering, and `scripts/verify.sh`, retaining
   raw failures and evaluator receipts. Review the final diff independently.
5. Follow AGENTS.md's standing background release/install rule: validate the
   release, keep a timestamp/hash-labelled recovery copy, sign and atomically
   replace the resolved ca executable, and verify signature/hash/version/help.
   Leave running processes alone.

## Final implementation

- `src/model_catalog.rs`: native compatibility 0.162.1 and exact official-route
  Sol fallback, with live/hidden/proxy isolation retained.
- `src/provider_contract.rs`, `src/agent/provider.rs`: exact Responses Sol
  capability/serialization regression, images/tools and five supported reasoning
  levels. API context is 1,050,000/default medium; native Codex is
  272,000/default low; explicit profiles retain precedence.
- `src/main.rs`: queued scoped/full discovery, separate global loaded state,
  identity preservation and generation invalidation after provider/auth edits.
- `src/tui/input.rs`, `src/tui/input_tests.rs`, `src/tui/render.rs` and five
  provider snapshots: provider-list refresh, provider-specific first visit,
  pending full discovery and Enter matching the displayed clamped row.
- `tools/provider_picker_pty.py`: three disposable real keyboard workflows,
  exact selected-route requests, saved defaults and terminal restoration.
- User guide, continuity, testing, next steps and index record the controls,
  evidence and actual final install outcome.

## Progress and evidence

- Before-images: `scratch/provider-picker-fix/before/`.
- Focused baseline: six input failures, two refresh-state/identity failures and
  four Sol/version/fallback failures are retained in that evidence directory.
  The initial live-test compilation failure (test fixture argument) is retained;
  the corrected baseline catalog-only test succeeds and returns five visible
  IDs, omitting Sol (`live-before-retry.log`).
- Actual baseline PTYs in `scratch/providers/` independently fail provider-list
  R, first full discovery after scoped success, and full discovery opened while
  scoped refresh is pending. Each child exits cleanly and restores termios.
- Repairs preserve the existing working selection/request-routing cases; the
  held-out terminal workflow includes catalog row insertion/removal, delayed
  refresh with visible typing/resize, provider defaults and HTTP-failure retry.
- Independent review identified that delaying queued discovery must still
  invalidate an in-flight result when provider configuration changes. Explicit
  generation invalidation and regressions are part of the repair.
- Candidate focus passes: 20 catalog, 134 input, six catalog-refresh, three
  Sol-matching and one MultiRun test, plus both exact provider-list render cases.
  The three review regressions fail before generation invalidation, with their
  unmodified failed logs retained. Full verification binds source digest
  `dcffe182d67965dc3857a65415a40e548032e6c1fbe79dbc6d3186d5a0c9db27`.
- Catalog-only authenticated comparison: old version returns five visible
  models without Sol; new version returns eight including `gpt-6.1-sol`,
  `gpt-6-sol` and `gpt-6-luna`. The picker matches the returned identifiers.
- The `.agent/PLANS.md` path referenced by AGENTS.md is absent; this plan follows
  the existing repository ExecPlan format.
- Official model schema: https://developers.openai.com/api/docs/models/gpt-6.1-sol
  (reasoning low/medium/high/xhigh/max; default medium; image input).

## Bounded fixture correction

The first debug workflow receipt waited for `gpt-6.1-sol` while its own `beta-`
filter correctly hid that model. The only fixture correction moves the unchanged
visibility wait after clearing the filter; refresh/input/resize assertions and
exact selection/request/default checks remain intact. Independent review confirms
the ordering error. The corrected private workflow passes eight checks; scoped
and pending-scoped scenarios pass two checks each. All children exit 0 and
restore termios. Original failure, correction diff and passing debug receipts
remain under `scratch/providers/`. The first full gate was deliberately stopped
with an infrastructure-failure receipt before changing the frozen fixture.
The final gate runs against the corrected repository fixture; no Rust source
changed during this correction.
The final frozen source digest is
`910812a13bd46f6f7c45c0f08b78c6ba30bddb6ef4c97b80994b88c926abf86c`;
receipts are under `scratch/verify/BUG-268-final/`. Provider-guide review
clarifies selection preservation when a row remains available and account-visible
native models; provider controls match the actual input paths.

## Existing deadline-sensitive test failure

The frozen default matrix reports one failure in
`session::tests::failed_over_bound_legacy_migration_preserves_original_bytes`:
`WouldBlock` instead of `InvalidInput` at `src/session.rs:4362`. That source is
byte-identical to both passing DEV-267 baselines. Independent review finds the
pre-validation identity scan has a 25 ms deadline and can return `WouldBlock`
before the oversized-record rejection. This is a timing inference, not evidence
of a data race or a provider regression. The original failed gate is retained.
The follow-up uses the unchanged exact test and complete failed default matrix;
no assertion, deadline or session implementation is relaxed. Passing stages
for the same frozen source/artifact may be reused under AGENTS.md's explicit
completion-check reuse rule. Final results below must distinguish the original
unattended gate from any passing follow-up.

## Frozen-source validation results

The complete final unattended gate finished with status **failed**, solely for
the default-matrix session case recorded above. Its original report, evaluator,
logs, manifests, runtime receipt and signed candidate remain unchanged.
Preflight, all 68 workflow contracts, formatting, strict workspace/all-target/
all-feature lint, the complete all-feature matrix (4,016 passed, zero failed,
44 ignored, zero filtered across 28 targets), release build and all eight
release runtime cases passed. The original default matrix has 3,866 passed,
one failed, 43 ignored, zero filtered across 27 targets.

The first unchanged exact session retry passes. It is diagnostic only; the
entire unfiltered default matrix was then rerun using the original command,
source and evaluator criteria. Its separate receipt is
`scratch/verify/BUG-268-default-rerun/retry-receipt.json`.

All three actual provider keyboard workflows pass on the retained signed
release: eight workflow checks plus two scoped and two pending-scoped checks.
There are 15/2/3 captured frames and 7/1/1 exact provider route POSTs respectively.
Every child exits 0, restores its original terminal settings and reports zero
fixture errors. Receipts: `scratch/providers/candidate-release-workflow.json`,
`candidate-release-scoped.json`, and `candidate-release-pending.json`.
The signed candidate hash is unchanged before and after those workflows:
`bcc3b9e6588c5b9a2973d816f4a4b6f723395deb17208b295b8ab253775190d8`.

This proves macOS terminal behavior with disposable loopback providers and
actual authenticated native catalog identifiers. No paid live inference,
other-platform behavior or long-soak result is claimed.

## Follow-up environment correction

The first unchanged default follow-up passes the original session case but
introduces a scratch-runner environment error: an explicit `CARGO_TARGET_DIR`
points nested sandboxed fixture builds at the repository target instead of
their own disposable workspace target. The existing jobs assertion fails with
`Operation not permitted` opening the repository `.cargo-build-lock`; that
assertion passes in both original matrices. The raw failing retry and its
recipe are preserved. Correct only that scratch environment override to match
the original runner; retain source, sandbox, assertions, deadlines and matrix
criteria. The next complete default receipt must be recorded separately.

The first follow-up completes all 27 targets with 3,865 passed, two failed,
43 ignored and zero filtered. In addition to the scratch target-directory
error, an existing stateful Git clean-filter count assertion sees one call
rather than two. Independent read-only loose-object inspection confirms that
the actual committed blob contains exactly the approved bytes. The filter
call count's timing cause remains unproven; the unchanged case passes both
original matrices. The corrected follow-up's isolated session diagnostic again
returns `WouldBlock`; it stops before any full matrix. This establishes
intermittence, without proving a cause.

Independent review recommends one final predeclared full default matrix using
the corrected environment, without exact-diagnostic gating or automatic
retries. The full command and strict unchanged evaluator criteria still require
all 27 targets, 3,867 passed, zero failed, 43 ignored, zero filtered, identical
ignored names/reasons and source/evidence hashes. Final outcome belongs in
`scratch/verify/BUG-268-default-final-matrix/retry-receipt.json`; a failure must
remain an acknowledged limitation, never a fabricated combined passing matrix.

## Final complete default outcome

The predeclared final unchanged default matrix **passes**, exit 0, in 747.782 s:
3,867 passed, zero failed, 43 ignored, zero filtered across all 27 expected
targets. The strict original evaluator's target inventory, per-target counts,
ignored names/reasons and nested-child handling match. Source before/after
remains `910812a13bd46f6f7c45c0f08b78c6ba30bddb6ef4c97b80994b88c926abf86c`;
criteria digest remains
`1110f9b889482d40884bfe1cab930d88eaa9288bde0cf14314bc44ec4943b068`.
The session case, sandboxed Cargo case and Git-filter case all pass unchanged
in this complete run. Original failure receipts remain failed and preserved;
the original full gate is not relabelled as a pass. Reuse its other seven
passing stages for this same frozen source and signed artifact under the
explicit AGENTS.md completion-check rule.

## Completed release and installation

Independent assessment confirms the final raw default matrix, unchanged
criteria/source, all expected targets and ignored cases, all original protected
evidence, prior failed receipts and signed candidate hashes. The all-feature
matrix's 4,016 passes and the other seven passing gate stages remain valid.
No validation concern requiring a provider repair remains.

Background release/install session **86127** completes exit 0. It performs the
standing all-feature workspace release build (0.94 s), signs the result and
refuses installation unless its bytes match the retained signed/tested candidate.
It stages on the destination filesystem, preserves the prior binary and
atomically replaces `/Users/aji/.local/bin/ca`, then signs it at that path.
The independent post-install readback verifies built/candidate/installed hashes,
0755 mode, owner `aji`, valid signatures, installed `ca 0.1.0` and help exit 0.
Installed SHA-256:
`bcc3b9e6588c5b9a2973d816f4a4b6f723395deb17208b295b8ab253775190d8`.
Recovery:
`/Users/aji/.local/bin/.ca-release-stage.5g1cYK/ca.recovery-ORC258-20261010T022641Z-53dcc22ab781b93c251b56e487bc80d56d5459b26df40ea03be34255ffdf8ac0`;
its verified hash matches the prior installed release.

Build/install log: `scratch/provider-picker-fix/release-install.log`.
Independent installed readback: `scratch/provider-picker-fix/installed-readback.json`.
The source digest is still the frozen 1,111-input digest after installation.
No real config/session edits or running-process interruption were performed.
Start a new CA process, open `/provider` or `/model`, use arrows/filter/Enter
to select, and Shift+R to refresh.
