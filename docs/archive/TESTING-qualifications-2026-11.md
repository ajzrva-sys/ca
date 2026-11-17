# Archived test qualifications

Moved from `TESTING.md` on 2026-11-13. These gates are historical. The installed version is in `CONTINUITY.md`.

## Historical qualifications

The sections below record past gates. They are not a claim about the binary in `CONTINUITY.md`.

## BUG-341 real DeepSeek qualification

Final gate `scratch/BUG-341/full-gate-release/report.json` and independent
`full-gate-stage-review.json` pass. Formatting, strict workspace/all-target
all-feature Clippy, 126 Python workflow tests, 4,976 default Rust tests and 5,132
all-feature Rust tests passed. Opt-in tests remain skipped: 45 default / 46
all-feature. All 9 release runtime cases and independent evaluation pass.
The final reviewed source identity is
`3c17faab13c5bd61e42cf09681853b271fcfe99b5d5c2fe8cf2386cd3b29802d`;
the gate's narrower inventory is
`78549cba3c02c4cd5c666b571e953883ca9d63f243e830c3085fe73509a67f8b`.
Their file sets match except for the separately verified `clippy.toml`.

One signed 0.1.99 artifact was built and installed:
`bae74cc8d904bdf07784a06ccd820bb112639dedad39165408f635e9419bd128`.
Real native DeepSeek acceptance uses ordinary CA from `/Volumes/projects`, saved
provider configuration, disposable HOME/native credentials and normal tools and
permissions. No mocked provider, forced model answer or manual control was used.
The release candidate ran `testing filter. list ~/projects/filtertest` in 63.987s
with 5 HTTP200 requests; the installed entry ran `testing filter. list ~/projects`
in 120.626s with 8 HTTP200 requests. Each used one Send, zero pre-Send requests,
automatic saved graph completion and local restored listings. Preview times were
671.848ms and 692.352ms respectively. Wide/narrow `/filter` inspection added no
provider requests. Both paths also passed independent debug-native audits.

Evidence: `scratch/BUG-341/qualification.json`, `real-candidate-filtertest/`,
`real-installed-final/`, `installation.json`, `protected-readback.json` and
`source-final-readback.json`. Earlier failures and interrupted gates are retained,
not counted as passing. The final gate covers the last test-only lint correction;
production source is unchanged from the successful final debug runs. No
cross-platform, universal-secret-detection or general provider-effectiveness
claim is made; ignored opt-in tests and production-session reuse are not covered.

## BUG-340 focused qualification

`python3 scripts/check-change.py --selector filename_ --selector task_filter
--selector structured_output --selector headless_contract --selector automatic_tasks
--all-features --release` passed 116 tests, formatting, strict all-feature bin
Clippy and the corrected release build. Final receipt:
`scratch/check-change/runs/20261109T224539Z-49203/report.json`; stable source digest
`23bd88599b6ce1fb144cf49d62504c21e38ce7fd11af207273e80c6baccd7a8f`.
Independent combined source review and final native-schema review found no blockers.

Coverage includes denied-descendant recovery, bounded duplicate-heavy discovery,
partial-census exact restoration and collision rejection, local-only coverage
notices, narrow/wide render visibility, exact native automatic-map schemas across
four wire formats, caller/mutated-schema rejection and private report restoration.
The original literal-alias test failure and earlier native fixture/schema failures
remain in `scratch/BUG-340/` and the BUG-340 plan. This bounded repair reuses FLT-338
workspace coverage for unchanged behavior; it does not claim a new full gate.

Actual-volume native acceptance passed on the exact signed candidate with
Suggestions and installed 0.1.98 with Private values:
`scratch/BUG-340/native-candidate-controls/receipt.json` and
`scratch/BUG-340/native-installed/receipt.json`. Each proves zero pre-Send traffic,
one Send, one planner, three requests total, native directory listing with issued
child references, exact saved workspace/read-only grants and authoritative saved
completion. Coverage and pending refresh are visible at 120×36 and 70×30.
Installed preview timings were 454/455/491 ms. The final harness uses the public
`.` workspace reference and native tool-success evidence, while admitting only
the existing bounded graph clarification control alongside list_dir. Earlier
fixture failures are retained; no compiled source changed during their correction.

## BUG-339 focused qualification

`python3 scripts/check-change.py --selector path_completion --release` passed
10 completion/input/render tests, formatting, strict affected-bin Clippy and one
all-feature release build. Receipt:
`scratch/check-change/runs/20261109T215941Z-20667/report.json`; source digest
`45f0e4596ad3598ecd9138e973eb8a81e5b3a1f8be0a83ffc3c6c0900c63bddf`.
Independent source review found no blockers. Existing full workspace coverage
from FLT-338 is reused for unchanged behavior; no new full-gate claim is made.

`scratch/BUG-339/path_completion_pty.py` reproduced the 0.1.96 zero-match popup
and passed with the signed candidate and installed 0.1.97 at
`scratch/BUG-339/native-{baseline,candidate,installed}/receipt.json`. The exact
draft `testing filter. list ~/projects` now shows the linked folder; Tab retains
`~/projects/` and lists children. Linked files complete; broken links are omitted.
Wide/narrow views, unchanged source/config/links, zero provider requests, clean
exit and terminal restoration passed. No external provider test was needed.

## ISO-337 selectors that passed locally

- `exact_file_grant_reads_that_file_and_rejects_siblings_and_prefixes`
- `directory_grant_rejects_symlink_escape_and_parent_components`
- `worktree_rebase_keeps_one_file_and_rejects_another_root`
- `child_execution_enforces_prepared_path_command_network_and_job_owner_scopes`
- `nested_spawn_inherits_subset_and_obeys_depth_limit`
- `optional_child_worktree_has_owned_branch_and_truthful_receipt`
- `admitted_children_keep_frozen_ceiling_and_observe_live_tightening`
- `admitted_child_environment_drops_credential_overlays`
- `experiment_environment_is_closed_and_redacted`
- `installed_skill_runs_after_its_source_checkout_is_removed`
- `catalog_request_snapshots_cover_every_template_without_secrets`
- `admission_rejects_bypass_redirect_and_sibling_use`
- `flag_without_a_spec_creates_no_authority`
- `confirmation_is_not_a_development_plan_and_cases_are_single_use`
- `failed_publication_does_not_repeat_a_seen_effect`
- `ledger_is_metadata_only_idempotent_and_attributes_exact_digests`

These five selectors passed on 2026-11-08 after the sandbox-root repair, with
`TMPDIR` under `/private/var/tmp`:

- `child_responses_restores_local_arguments_and_filters_retrieved_continuation`
- `narrow_worker_await_wire_exposes_only_host_issued_completion_and_progress`
- `narrow_worker_host_native_edit_retains_filename_privacy_and_mutation_receipt`
- `narrow_worker_managed_private_source_establishes_host_view_without_model_read`
- `child_execution_enforces_prepared_path_command_network_and_job_owner_scopes`

## Gate

FLT-338 qualification is recorded in `scratch/FLT-338/qualification.json`:

- Focused behavior/render coverage: 78 tests, formatting and strict lint passed
  in `scratch/check-change/runs/20261109T203500Z-61529`; the subsequent help-only
  correction passed both selectors, formatting and lint in
  `scratch/check-change/runs/20261109T205258Z-58006`.
- `scratch/FLT-338/full-gate-final` passed formatting, strict workspace lint,
  default tests (4,952 passed; 45 ignored), all-feature tests (5,108 passed;
  46 ignored), one release build and candidate signing. Parsed totals exclude
  nested child summaries. The original gate remains failed because its legacy
  runtime fixture used raw paths where the default filter requires references.
- Only four Python fixture/evaluator files changed afterward; compiled source
  and the signed candidate are unchanged. All nine corrected runtime cases
  passed in `scratch/FLT-338/runtime-repair-final`. Independent runtime evaluation
  passed in `scratch/FLT-338/runtime-repair-final-evaluation.json`. The correction
  retains actual file bytes, issued-reference binding, exact candidate digest,
  six requests and the sole independent report. All 82 affected Python contracts
  passed; final verifier defect/restoration self-test passed in
  `scratch/FLT-338/verifier-selftest-qualified`.
- `python3 tools/filter_review_pty.py --binary <candidate-or-installed-ca>
  --output <new-directory>` passed for the exact candidate and installed binary
  in `scratch/FLT-338/native-release` and `scratch/FLT-338/native-installed`.
  Each used 196 history messages, seven provider requests and two planners:
  no pre-Send request, both presets, cancellation, edited suggestions, automatic
  execution, native path restoration and two visibly fresh pending updates in
  wide/narrow views. Source bytes and fixture settings stayed unchanged.

The earlier failed gates and fixture attempts are retained in the completed
[FLT-338 plan](docs/exec-plans/completed/FLT-338-automatic-filter-preview.md).
Evidence uses loopback providers; paid/external provider behavior and ignored
opt-in cases are unverified. Previous installed baseline gate:
`scratch/verify/20261109T085617Z-78373` (ca 0.1.95).

## Historical install record (BUG-340, ca 0.1.98)

This section is the BUG-340 installation evidence. The current installed version is in `CONTINUITY.md`.

ca 0.1.98 is installed at `/Users/aji/.local/bin/ca`. SHA-256
`1876595a1750676b08aa8bedb821a762beb0c2e354d6c63052d8b3c7857177b6`.
Candidate, stage and installed hashes match; signature, ownership/mode,
`--version`, `--help` and installed terminal acceptance passed. Recovery path and
readback are in `scratch/BUG-340/installation.json`; background install job 21418
completed successfully, log `scratch/BUG-340/install.log`. Of 4,459 tracked
metadata entries, only the live dashboard-presence directory and one presence
record changed; saved configuration/session metadata remained unchanged.
