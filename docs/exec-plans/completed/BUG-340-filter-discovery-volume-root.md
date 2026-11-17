# BUG-340 — Filter preview from a volume root

Status: COMPLETED — signed ca 0.1.98 installed and read back. The reported failure was
“Workspace filename discovery failed; request was withheld” on ca 0.1.97.

## Observed acceptance failure

The user's live CA process has cwd `/Volumes/projects`. A metadata-only traversal
reproduces EPERM at `/Volumes/projects/.TemporaryItems` after 230 directories and
3,680 entries. Skipping inaccessible descendants exposes a second required
blocker: after 40 seconds the volume has yielded 5,091,082 entries, 416,971 unique
names and 599,714 directories, with work still pending. This exceeds the current
262,144-unique-name census limit. Neither failure concerns path autocomplete.
The original raw filename error collapses all read-directory errors.

Normal acceptance: enter `testing filter. list ~/projects` from the volume root;
prepare a local editable preview, clearly state unavailable/incomplete discovery,
then Send once and automatically execute a native directory listing. No task
provider request precedes Send. Verify normal entry on the candidate and the
installed binary with disposable configuration/session state and loopback only;
the actual workspace is read-only.

## Scope

- Keep root/shallow discovery and existing alias restoration. Treat inaccessible
  descendant folders as a local coverage gap, not a global request failure.
  An unreadable root, invalid names/references and issued-registry limits remain
  errors. Previously issued unavailable paths retain historical restoration but
  cannot execute as live paths.
- Bound recursive discovery to 25,000 descendant entries, including duplicates.
  Preserve the gathered census and disclose the unscanned coverage. Do not add a
  timing-dependent cutoff or raise storage limits. Explicitly encountered paths
  continue to use on-demand masking and exact restoration.
- Incomplete discovery cannot establish global basename uniqueness. Bare names
  remain descriptive; exact full-path references retain execution semantics.
  Check newly encountered literal alias-shaped names before projection.
- Keep coverage process-only and revision-aware; no session-format change.
  Preview and current-task inspection display the actual coverage, never send
  coverage diagnostics as provider input.

## Ownership and validation

Runtime owner: `src/privacy/filenames.rs`, `src/privacy/mod.rs`, associated private
tests. Root: `src/filter_review.rs`, preview render regression, docs and release.
Native acceptance owner: scratch-only harness/evidence. Independent reviewer:
combined filtering/collision/coverage review before final source freeze.

Focused filename, task-policy, admission and render checks, formatting and strict
lint; reuse unchanged protocol/execution full-workspace baseline. Verify bounded
discovery on the representative real volume, repeated previews, readable sibling
masking, unavailable-root refusal, permission recovery, partial-census path
restoration and on-demand collision rejection. Keep all failure evidence.
Run one release build after freeze, then recoverable signed install and installed
entry readback. Preserve production settings/session state and existing processes.

## Progress

- [x] Actual entry point and both discovery blockers identified.
- [x] Baseline source copied from the verified 0.1.97 checkout.
- [x] Bounded discovery, explicit coverage and regressions implemented.
- [x] Independent combined review and focused qualification.
- [x] Candidate and installed normal-entry acceptance.

Evidence: `scratch/BUG-340/qualification.json`. Signed 0.1.98 is installed.

## Retained qualification evidence

- The installed 0.1.97 failure was reproduced from actual `/Volumes/projects`
  with disposable state and an OS-enforced write-denial profile for that root.
  `native-baseline-confirmed/receipt.json`: exact discovery failure in 501 ms,
  zero provider requests, clean exit and terminal restoration. The first native
  fixture wait timed out because the failed preview retained its loading copy;
  that non-passing attempt remains in `native-baseline/`.
- A metadata-only bounded traversal visited 25,000 descendant entries and all
  14 root entries in 0.305 seconds, with 17,181 unique names and four inaccessible
  system directories. No other invalid-name or filesystem error appeared:
  `metadata-bounded.json`. Native application timing is still required.
- Combined source review found no unresolved blocker before focused checks.
  Root version advanced once from 0.1.97 to 0.1.98; candidate results follow below.
- First focused run `scratch/check-change/runs/20261109T222446Z-2671` passed
  91 tests and failed the unchanged literal-alias namespace regression. The new
  collision check observed a stale lookup after the unused namespace was renamed.
  Rebuild that derived lookup immediately after renaming, before registration;
  preserve collision checks and the existing test. The repair passed independent
  review. The first failed run did not run formatting/lint or build a release.
- Corrected focused run `scratch/check-change/runs/20261109T222848Z-11769`
  passed all 92 tests, formatting, strict all-feature bin lint and a release
  build. Signed candidate SHA-256:
  `45a2911f75305d1fd6dd06be1df4c103a2e995b415eb152c1948989212515db4`.
- `native-candidate/` remains failed: initial wide/narrow preview worked, but
  the fixture accepted a partially repainted preset switch and captured pending
  output as completed. Its successor explicitly captures pending then ready.
- `native-candidate-confirmed/` passed both complete previews but timed out after
  Send with zero provider requests. The saved automatic author workflow failed
  on an immutable schema privacy check. Exact cause: the standard host task-map
  schema URI `http://json-schema.org/draft-07/schema#` contains a discovered
  basename `http` from `.mimocode/node_modules/effect/src/unstable/http`. Both
  task term lists are empty; preset choice did not cause the collision.
  Diagnosis: `native-candidate-confirmed/schema-diagnosis.json`.
- Required repair extends the existing closed native-schema binding with the
  exact host automatic-task schema. Its native constructor accepts no caller
  JSON, retains task-graph preflight, and must recognize the exact schema before
  projecting descriptive strings. Dynamic/caller schemas and report payloads
  retain all privacy checks. No manual-workflow/schema allowlist expansion.
  Source owners additionally include `src/headless_contract.rs` and
  `src/automatic_tasks.rs`. This requires new focused schema tests and rebuilding
  the candidate; the first artifact is retained and has not been installed.

- Final schema correction passed independent review. Exact native-schema equality,
  caller-schema rejection, report filtering/restoration, graph preflight and
  explicit stop rules remain intact. Final focused run
  `scratch/check-change/runs/20261109T224539Z-49203` passed 116 tests, formatting
  and strict all-feature bin lint; corrected release build passed in 186 seconds.
  Tested source digest:
  `23bd88599b6ce1fb144cf49d62504c21e38ce7fd11af207273e80c6baccd7a8f`.

- Corrected signed candidate SHA-256 is
  `1876595a1750676b08aa8bedb821a762beb0c2e354d6c63052d8b3c7857177b6`.
  `native-candidate-final/` reached the provider after 475 ms and 479 ms previews,
  but the fixture expected an executable root alias absent from an unattached
  plain task. Its HTTP 500 caused one identical-body retry; this is not duplicate
  Send. Existing tool guidance and `FilenamePrivacy::resolve_path` support public
  `.` for the workspace. Correct the fixture to that supported spelling and
  validate root binding plus full issued child references after native listing.
  Retain the failed receipt and old harness; compiled source is unchanged.

- `native-candidate-qualified/` retained a second fixture failure: directory
  artifact evidence rejects `.` even though native `list_dir` accepts it. Use
  the current `tool_success` completion gate for the requested listing. It is
  host-observed only after an actual successful tool, and the final receipt must
  contain a satisfied authoritative pass. This is the appropriate listing
  criterion; no Rust policy change, extra tool, command or rebuild is needed.

- `native-candidate-listing/` retained the last fixture failure: graph workers
  always expose bounded `request_graph_clarification` alongside their native
  grants. Independent source review confirmed it is a read-only interaction
  control. Require that exact two-tool set and retain list_dir-only saved grants.
  No source build changed. Final harness SHA-256:
  `c16d68b27acce9e1be7e4177249a725c36ac6ede93904d56fedd82ee99c328ce`.
- `native-candidate-controls/` passed with Suggestions; `native-installed/`
  passed with Private values on the same signed artifact. Each performed one
  Send, one planner, one native listing and three requests total, with no prior
  traffic. Saved workspace is the exact volume, grants are read-only/list_dir,
  every returned child has an issued path, and the single attempt has a satisfied
  host observation and saved completion. Independent native review found no
  blocker. Initial/second candidate previews: 461/470 ms; installed default,
  Suggestions and return-to-default previews: 454/455/491 ms. Wide/narrow coverage
  and pending-to-ready mode refresh are visibly present.
- Installation job 21418 completed. Candidate/stage/installed hash, destination
  signing, mode/owner, version/help and installed native readback all passed.
  Previous 0.1.97 is preserved recoverably; installation.json records its path.
  Compiled source still exactly matches the tested inventory. Of 4,459 production
  metadata entries, only the live dashboard-presence directory and one presence
  record changed. Saved config/sessions and existing processes were preserved.

## Result and limits

The reported normal-entry failure is repaired and installed. Discovery explicitly
reports partial coverage: unscanned bare names may remain readable. An unreadable
root, invalid paths/names and issued-alias storage limits still stop transmission.
Exact encountered paths retain masking/restoration. This repair passed 116 focused
all-feature tests, formatting, strict bin lint, release, independent review and
actual-volume native acceptance. Existing FLT-338 workspace evidence is reused
only for unchanged behavior; no new full workspace gate or real external-provider
run is claimed. All nonpassing source/native attempts remain retained above.
No BUG-340 implementation or installation work remains.
