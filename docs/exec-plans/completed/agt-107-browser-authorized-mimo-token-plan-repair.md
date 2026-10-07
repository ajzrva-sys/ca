# AGT-107 — Browser-Authorized MiMo Token Plan Repair

Status: COMPLETE
Owner/session: Codex desktop `/root`
Last updated: 2026-08-15

## Goal

Make CA classify browser-authorized Xiaomi MiMo credentials by the returned
credential and exact endpoint, repair the saved Singapore Token Plan route
without reauthorization or changing the OpenAI Codex parent, expose the
official off-peak discount accurately, and prove one live simple delegation
from Codex to MiMo.

## Non-Goals

- Do not modify or copy implementation source from `oracles/` or `reference/`.
- Do not make paid requests while implementing or running automated tests.
- Do not make OpenAI Codex OAuth automatically delegation-eligible.
- Do not change Linux or FreeBSD provider behavior beyond portable tests.

## Evidence and Current Behavior

- `src/main.rs` currently sends successful MiMo browser authorization directly
  to `provider_connect::connect_direct`, so Token Plan billing metadata is never
  collected.
- `src/tui/render.rs` labels browser authorization as PAYG compatibility.
- `src/pricing.rs` correctly requires a `tp-` credential, an exact regional
  Token Plan URL, complete confirmed billing metadata, and fresh official
  pricing before automatic routing.
- Offline doctor therefore reports the saved Singapore MiMo credential as
  connected but ineligible because its Token Plan allocation metadata is
  missing.
- A previously authorized live simple delegation reached the router and failed
  before MiMo traffic because no route qualified.

## Scope

Allowed files: provider-authentication, configuration, pricing, doctor, usage,
TUI, focused tests/snapshots, and project documentation needed by this task.

Forbidden files/areas: `oracles/`, `reference/`, unrelated dirty worktree
changes, and secrets in logs, fixtures, snapshots, or documentation.

Runtime state affected: after all offline gates pass, back up and atomically
replace `/Users/aji/.local/bin/ca`, then update only the existing MiMo billing
metadata and make one explicitly authorized live delegation request.

## Design and Invariants

- Classify MiMo from both credential form and exact endpoint. A `tp-` key plus
  one of the three documented Token Plan endpoints is Token Plan; a non-`tp-`
  key plus an ordinary Xiaomi endpoint is PAYG; mixed forms fail closed before
  traffic or persistence.
- Existing saved Token Plan repair reuses the credential reference, endpoint,
  models, and defaults. It collects plan metadata, probes once with a transiently
  resolved credential, and atomically writes only billing metadata after success.
- New browser Token Plan credentials remain process-only until metadata is
  confirmed, then credential and billing state commit transactionally. Stale or
  cancelled setup generations discard the pending secret.
- Canonical off-peak billing remains 16:00–24:00 UTC at 0.8x. User-facing copy
  also reports 9:00 AM–5:00 PM PDT / 8:00 AM–4:00 PM PST. Forecast routing stays
  peak-only; only requests wholly within the UTC interval receive the discount.
- No diagnostic, event, error, snapshot, or debug representation contains a
  credential or authorization result.

## Milestones

### M1 — Characterization and classification

- Add strict endpoint/credential classifier tests, including all regions and
  mismatch failures.
- Add failing TUI and persistence tests for saved repair and browser Token Plan
  metadata collection.

### M2 — Authentication and persistence repair

- Add saved-credential setup selection and metadata-only commit path.
- Delay browser Token Plan commit until metadata confirmation; retain PAYG
  compatibility without delegation eligibility.
- Refresh route clients without changing the active Codex parent.

### M3 — Pricing presentation and diagnostics

- Correct provider setup/doctor wording and expose the 20% off-peak terms in
  setup, pricing status, forecasts, usage, and documentation.
- Add deterministic render and arithmetic coverage.

### M4 — Offline validation and delivery

- Run focused tests, formatting, strict Clippy, full all-feature tests, and the
  macOS arm64 release build.
- Back up and atomically install the binary, repair the saved Pro annual $528
  Singapore billing metadata, verify doctor eligibility, and run one authorized
  simple delegation.

## Acceptance Criteria

- Browser and manual MiMo credentials are classified consistently without
  leaking secret material.
- Saved Token Plan repair does not reauthorize and changes only MiMo billing
  metadata.
- OpenAI Codex remains the configured parent/default.
- Doctor reports the MiMo route eligible with fresh pricing and complete
  Token Plan metadata.
- A live `complexity="simple"` child selects `mimo/mimo-v2.5-pro`, succeeds,
  records `Subagent` usage, and is never retried on another provider after start.

## Validation

Focused: PASS. Classifier, pricing/accounting, provider-connect rollback,
billing-only persistence, offline doctor, setup input, usage, and all 118 TUI
render tests passed. No focused test contacted a provider.

Completion: PASS. `cargo fmt --check`, strict all-target/all-feature Clippy,
`cargo test --all-targets --all-features --no-fail-fast`, and
`cargo build --release --all-features` all passed. The final test run recorded
1,329 main tests passed (one ignored manual Chromium case), 115 experiment tests
passed (one ignored external fixture), and 36 CLI tests passed.

Manual/visual: PASS. The installed TUI recognized the saved Singapore Token Plan
without sign-in, collected Pro / annual / $528, made one successful isolated
MiMo probe, and retained OpenAI Codex as the parent. Installed offline doctor
then reported one eligible delegation provider and the MiMo route as `USD Token
Plan ready`, `standard`, and `routing eligible`. One authorized headless live
run selected `mimo/mimo-v2.5-pro` and returned `AGT107_MIMO_OK`.

Blocked checks: none.

## Progress

- [x] Reproduced the router refusal and identified missing browser Token Plan
  billing metadata as the remaining failure.
- [x] Add characterization tests and strict MiMo classification.
- [x] Implement saved/browser Token Plan metadata flows.
- [x] Update off-peak presentation, doctor output, and documentation.
- [x] Pass offline validation and build release binary.
- [x] Install, repair runtime metadata, and complete live acceptance.

## Decisions

- Decision: repair the saved route as Singapore, Pro, annual, official-list
  allocation of $528.
  Evidence: explicit user selection and the existing saved Singapore endpoint.
  Consequence: no credential re-entry or browser authorization is required.
- Decision: retain OpenAI Codex as the active parent.
  Evidence: explicit user selection.
  Consequence: provider repair must not invoke model/default selection.
- Decision: forecasts use peak MiMo credits even when the current wall clock is
  off-peak.
  Evidence: deterministic fail-closed AGT-106 routing contract.
  Consequence: the discount is disclosed but cannot make a route newly eligible.

## Surprises and Discoveries

- MiMoCode browser authorization returns both the credential and its selected
  base URL; browser authorization itself does not imply PAYG billing.
- Reading saved Keychain secrets in offline doctor can block on macOS security
  mediation. Doctor now uses a separate metadata-only provider snapshot and
  never resolves Keychain values; strict secret/endpoint classification remains
  at the request and repair boundaries before traffic.

## Recovery and Rollback

- Preserve the original config bytes until a probe succeeds and use the config
  store's atomic update boundary for billing-only persistence.
- Retain the prior installed binary under a timestamped backup next to
  `/Users/aji/.local/bin/ca`; restore it with an atomic rename if live acceptance
  exposes a regression.
- Do not use destructive Git recovery commands in the dirty worktree.

## Completion Record

Changed files: `src/mimo_auth.rs`, `src/provider_connect.rs`, `src/config.rs`,
`src/pricing.rs`, `src/doctor.rs`, `src/usage.rs`, `src/app.rs`, `src/main.rs`,
`src/tui/input.rs`, `src/tui/render.rs`, focused tests/snapshots, and the setup,
pricing, delegation, architecture, testing, continuity, index, and task-queue
documentation. `oracles/` was not modified.

Commands and results:

- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features --no-fail-fast`: PASS (1,329 main,
  115 experiment, and 36 CLI passed; two explicitly ignored external/manual
  cases).
- `cargo build --release --all-features`: PASS.
- Installed `/Users/aji/.local/bin/ca` SHA-256
  `ede243f9de9465bdfb8c95b2585bc30ace9e5fd7972b88d106e7c7fee3a8fdc9`.
  The recoverable prior binary remains at
  `/Users/aji/.local/bin/ca.backup-agt107-20260815-1709` with SHA-256
  `e3d79cef89cb692e37871a1b751e5fce8475be52cd2e90dd0616cf4bb56ce65c`.
- Installed `ca --doctor`: PASS; default stayed
  `openai-codex / gpt-5.6-sol`, and MiMo became delegation-eligible.
- Authorized live run `efae409d-c1f5-4b04-8b13-27fb27ef7fe7`: PASS. The Sol
  parent called `task` once with `complexity="simple"`; CA routed exactly once to
  `mimo/mimo-v2.5-pro`, returned `AGT107_MIMO_OK`, emitted a peak-only Token Plan
  forecast with the 20% off-peak discount excluded, and exited 0. The routed
  request used the `Subagent` purpose/accounting path; there was no fallback
  retry and no credential appeared in output.

Remaining risks: Xiaomi still exposes no supported account-plan API, so tier,
term, and paid USD remain user-confirmed metadata. Non-USD plans and PAYG
credentials remain deliberately ineligible for automatic routing. Headless JSONL
reports the selected route and forecast but does not serialize the process-local
usage ledger row; `Subagent` classification is enforced in the shared runner and
covered by direct ledger tests.

Harness updates: strict credential/endpoint matrix tests; billing-only atomic
persistence and secret-free doctor tests; saved/browser/manual setup reducer and
wide/narrow snapshot coverage; exact discount, peak forecast, boundary, routing,
and usage-label tests; and mocked headless `Subagent` route/accounting tests.
