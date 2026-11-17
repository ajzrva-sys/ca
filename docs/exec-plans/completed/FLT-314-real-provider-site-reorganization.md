# FLT-314 — Real-provider website creation, rename and movement

Status: COMPLETE — installed ca 0.1.72 passes without `--filter` (2026-10-28).

## Outcome and requested workflow

Normal interactive entry uses the installed signed ca and native real
`openai-codex/gpt-6.1-sol`, without `--filter`. Saved structural/filename privacy
is active before input, with legacy Latin configured as a control. Disposable
runtime state mirrors the verified saved privacy switches; actual production
configuration is checked through opaque digests, without reading credentials.
ca alone resolves native authentication.

The original six-file HTML/CSS/entry JS/shared JS/SVG/PNG site spans private
folders and nested private destinations. Native write/read/delete tools create
extra.js, rename the stylesheet to renamed.css, move the entry script to a
nested deployment directory as entry.js, and move the page to a nested page
directory as overview.html. These are validated native copy/read/delete
operations, not atomic filesystem rename calls or automatic link refactoring.
The model updates incoming HTML links; ca restores/rebases outgoing references.

All seven final files match exact expected hashes. The three old paths are gone;
no unexpected files exist. All seven HTML/CSS/JS links/imports resolve to their
expected targets, including the stylesheet query/fragment, root-relative SVG,
relative PNG, original shared module and new helper module. Shared JS/SVG/PNG
hashes are unchanged, private source data is preserved, and no aliases remain
in local source.

## Repairs driven by the real tests

The first actual provider test exposed incompatible path vocabularies and
failed-listing replay. Filename mode now projects authored paths with its
executable canonical references; the separate private-batch binding path remains
unchanged outside filename mode. `[tool failed]` directory diagnostics are not
parsed as filename rows, and ordinary privacy masking still applies.

The next installed test created extra.js, then blocked because newly discovered
filenames rewrote an already-disclosed leaf in the earlier prompt. Existing
creation records now retain those public leaf spellings while private full,
absolute and explicit-relative paths remain aliased. A plain proposed leaf
containing an existing protected name is rejected before reserving a reference;
that collision cannot declassify an existing basename.

A focused rename-history test also reproduced source-link identity changes
when an original target is removed and the projection cache refreshes. Exact
already-issued (original spelling, source, target) records now retain historical
identity after removal. Native execution still requires a live target. No
immutable-history guard, transport protocol, session schema or approval rule
was weakened or changed.

## Verification and preserved failures

- Failure-first regressions reproduce both original blockers, the creation
  prefix change, and the removed-target history change. The collision check
  includes the actual proposed creation reference and unchanged registry.
- Final focused receipt:
  `scratch/check-change/runs/20261029T034116Z-52076/report.json` — 275 all-feature
  privacy/filter/provider-replay tests pass, two existing opt-in probes ignored;
  formatting, strict binary Clippy and source stability pass. Independent final
  source review: `scratch/FLT-314/independent-source-review-final.json`.
- Installed real run: `scratch/FLT-314/provider-installed-5/receipt.json` —
  104.143 seconds, 32 physical HTTPS requests, all HTTP200/completed. Thirty-one
  native calls: 13 reads, 11 listings, four writes, three deletes. One attempted
  PNG text read was rejected as non-UTF-8; the model recovered. Final marker,
  Done state, normal exit 0 and terminal restoration pass.
- Final trace: 3,276,964 bytes / 3,691 contiguous events. Starts, responses and
  finishes correlate by request/attempt, and extracted physical bodies match.
  Parent privacy scan: 55 private originals / 635 raw and encoded variants,
  zero leaks. Independent final audit passes: 55 originals / 667 raw and encoded variants,
  zero leaks; `scratch/FLT-314/independent-audit.json`. Its initial PNG-identity
  predicate expected a visible alt label that was correctly hidden; the corrected
  predicate matches the second image source token. The failed audit-helper
  attempt is retained; no product or acceptance requirement was changed.
- Two initial fixture-input errors sent zero requests (directory @attachments,
  then trailing punctuation adjoining an explicit @file). First focused launch
  could not run because the default offline cache lacked an existing dependency;
  the established private cache was reused successfully. All receipts remain.
- Failed provider run 3: eight HTTP200/completed requests, namespace/listing
  blocker, no site mutations. Failed run 4 on installed 0.1.71: 13 physical
  HTTP200/completed requests plus one locally rejected continuation; only the
  helper was created. These remain failed in their receipts and independent
  audits. The final run uses a fresh fixture with the same acceptance prompt;
  no completed operation was replayed against either failed fixture.

## Release and readback

Two distinct repaired source versions were built once each: 0.1.71 (172.017s,
background session 44210) and final 0.1.72 (146.535s, session 17438). No matching
artifact was redundantly rebuilt. The final source differs from the passing
focused source only in the required root Cargo version/lock bump; reviewed
functional hashes are unchanged. Candidate, staged and installed signing,
hashes, ownership/mode, version/help and installed workflow pass.

Final signed installed SHA-256:
`5db9551ac7633724c67d8bdeb8e2ab77ee514b0a7bf73f6498526c574c2527ae`.
Final recovery:
`/Users/aji/.local/bin/.ca-release-stage.VJfq41/ca.recovery-ORC258-20261029T034524Z-06ed3b7d3f54a2ddbd86e2eab2080356d675a972ef733878b07f276975481140`.
The earlier 0.1.70 recovery remains recorded in `install-receipt.json`.
Final build/install receipt: `scratch/FLT-314/release-72/install-receipt.json`;
job log: `scratch/FLT-314/release-install-72.log`.

All 112 production config/session metadata entries match across the entire task,
and actual configuration bytes retain their verified activation digest. Running
user sessions were left alone. Installed binary remains unchanged throughout the
final provider test, and its final digest matches the validated release.

## Capture limits and changed files

This is final application pre-send evidence, not remote packet capture. Ten
native encrypted reasoning replay fields in requests 23–32 remain opaque.
No inspectable plaintext or raw-frame omissions were detected. PNG text reading
was rejected; image pixels and browser rendering are not qualified. The test is
one provider/model on a synthetic local site with `--no-session`, not a
cross-version resume or cross-provider qualification. No new full workspace gate
was run for these bounded filename changes; focused coverage is reported as such.

Product changes: `src/privacy/mod.rs`, `src/privacy/filenames.rs`,
`src/privacy/site_tests.rs`, root `Cargo.toml`/`Cargo.lock`. Documentation:
`docs/CONFIGURATION.md`, this plan, `CONTINUITY.md`, `NEXT_STEPS.md`, `TESTING.md`,
`docs/.INDEX.md`. Helpers, before-images, failed attempts and receipts remain in
`scratch/FLT-314/`. No oracle/reference files, provider credentials, production
configuration or saved sessions were modified. No remaining FLT-314 work.

Final evidence index: `scratch/FLT-314/final-assessment.json`.
