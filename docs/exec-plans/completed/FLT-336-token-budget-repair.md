# FLT-336 — Repair graph token budgets

Status: COMPLETE (2026-11-08). Signed CA 0.1.88 installed and verified.

Normal entry is ordinary Agent submission, reviewed task map and graph workers.
Installed baseline is signed CA 0.1.87, SHA db7f9b5bc7…; its full workspace gate
and installed terminal/native-import receipts remain current unchanged-target
evidence. This checkout has no .git or .agent/PLANS.md; retain source inventories.

Observed failure: the first worker consumed 107,337 actual tokens across six
requests against a fixed shared 100,000 ceiling. Settlement rejected the final
48,463-token observation before storing it, leaving 58,874 saved. The planner
could configure request/cost limits but not total tokens. Starting worker briefs
were focused; later read/search evidence grew repeated request context. That
run explicitly disabled context pruning; do not silently override that choice.

The user explicitly selected KEEP THE FIXED 100,000 TOKEN CEILING. Preserve it,
the request/time limits and saved graphs/settings. Show the ceiling in review;
reduce repeated graph-worker context rather than sizing or raising the cap.
The earlier proposed task-sized budget was not implemented and is superseded by
this explicit user choice. The old replay fixture's pruning opt-out was set by
the earlier agent; honor actual opt-outs and qualify the normal enabled path.

- [x] Reproduce missing over-limit/in-flight accounting and equality admission.
- [x] Repair actual usage recording without allowing further blocked dispatch.
- [x] Bound graph-worker request context and visibly disclose the fixed ceiling.
- [x] Frozen independent review, focused checks and applicable completion gate.
- [x] Normal entry verification, one signed recoverable release/install/readback.

Use isolated state and saved evidence under scratch/FLT-336. Preserve all prior
failed/comparison receipts unchanged. No BSD contact, original ESXi edits,
credential inspection or production config/session edits. Do not claim completion
or refusal improvement without actual evidence. Record failed checks honestly.

## Implementation and frozen review

Known provider observations settle total/output/cost/unpriced counters before
checking limits, including late or concurrently dispatched responses after a
block. Admission rejects equality and an estimated protected input that cannot
fit. Fixed 100,000 remains unchanged, cannot be raised by a planner proposal, and
is shown in map review and planner/worker guidance.

Graph-only request projection ages whole successful read/list/search/glob bodies
in a cloned request, retaining newest batches, errors, skills, commands, media,
incomplete pairs and authenticated duplicate bases. Canonical worker history is
unchanged. Explicit pruning opt-outs are honored. Private receipt-bound bodies
are never sliced. Estimates and concurrent in-flight usage cannot guarantee an
invoice below 100,000; actual observed excess remains visible and blocks dispatch.
Planner requests retain their existing separate accounting.

Final frozen source 4197f1cb1e4b1ebf20f1ffbe56387bb9317d330738d4b42fa0f56bf9a75b8d0c
contains 1,223 files. Independent combined and final-delta reviews pass in
`scratch/FLT-336/review{,-final}.json`. Root version advanced once to 0.1.88;
the single release is installed with exact-artifact terminal readback.

## Development verification

- Failing-first accounting/equality regressions retained. Focused final receipt
  `scratch/check-change/runs/20261108T205842Z-79800/report.json` passes 51 tests,
  formatting and strict ca-bin Clippy against the final frozen source.
- Unchanged normal-TUI pressure fixture reproduces baseline0.1.87 failure:
 114,529 declared graph tokens, only 84,442 saved, total-token block before
 completion. Repaired execution records 66,245/66,245, completes4 tasks in 19 local
 requests,195 history messages, two visible pending updates, explicit retry,
 independent branch progress and active filtering with its window closed.
 These are deterministic synthetic provider charges proportional to request
 bytes, not real-model tokenization or paid-provider effectiveness.
- Earlier timing/UI-fixture failures remain failed. First full gate was aborted
 before release to add planner cap guidance; its receipt remains non-passing.
 Final `scratch/FLT-336/full-2` passes format, strict workspace lint, default
 and all-feature tests, the single release build, runtime and evaluation.

## Bounded real checks: no completed worker qualification

Two separate repair checks used copied evidence and debug 592a (execution fixes,
without the final planner guidance/version delta), not the final release. They
do not replace or revise the frozen original FLT-334 comparison. The unchanged
nine-test copied suite passes preflight under resolved/private temporary paths.

The first check was stopped before approval after review found a generated
input-reference path defect. An initial mistaken approval recommendation was
withdrawn; no approval marker or workers ran. The corrected fresh check used one
normal review revision to fix test-log/host-check lifecycle wording. It reached
900 seconds after 11planner requests; its proposed 22 graph requests exceeded the
remaining 19 under the global 30-request limit. The guard prevented approval.
No worker suite, final report, token-completion or refusal-reduction evidence is
claimed. Preserve both traces/maps/reviews/process receipts and do not make
further provider calls to chase a successful outcome.

The independent real-results audit is retained at
`scratch/FLT-336/real-results-review.json`. All 15 outgoing attempts selected
xhigh. Trace-known planner usage is 40,730 tokens for the first check and 102,606
for ten completed observations in the second; the eleventh attempt has no final
usage, so its full billing is unknown. Saved graph usage is zero because no
worker started. Planning is not free and is outside the existing worker ceiling.
Pre-install preservation confirms all 4,458 baseline metadata entries, 56 original
saved-source hashes and all 1,223 frozen source hashes unchanged.

## Changed files and verification entry points

Production changes are limited to `src/agent_graph.rs` (admission/settlement),
`src/context_pruning.rs` and `src/context_observation.rs` (graph request projection
and typed duplicate-basis protection), `src/agent/subagent.rs` (worker integration
and guidance), `src/automatic_tasks.rs` (planner guidance), and
`src/tui/render.rs` (review budget disclosure). `src/tui/render_tests.rs` adds
wide/narrow disclosure coverage. `tools/plan_workflow_pty.py` provides the usage
hook and new `tools/token_budget_pty.py` exercises actual terminal/native tools.
Cargo files contain only the root release version bump. User/configuration
guidance and repository handoff pages record the behavior and evidence.
`changed-files.json` binds the exact delta to the installed FLT-335 source.

```sh
python3 scripts/check-change.py --selector autonomy_budget --selector graph_worker_ --selector automatic_map_ --selector orc290 --selector provider_structured --selector delegated_xhigh
./scripts/verify.sh --output scratch/FLT-336/new-full-run
python3 tools/token_budget_pty.py /absolute/ca /absolute/new-pressure-output
```

## Final delivery

Full workspace gate and independent artifact audit pass for frozen 4197 source:
4,878 default and 5,034 all-feature tests, zero failures, 45/46 existing ignored.
Focused verification passes 51 tests, formatting and strict Clippy; its source
copy matches all 1,223 frozen files with only the helper clippy.toml added.

One signed release build (job 98725, `scratch/FLT-336/full-2.log`) produced 0.1.88.
Exact SHA-256 is 9f064b7e6fcda12cf915985877d30d9fbd1334818c938c783bcb018b99aefe9b.
Candidate and actual installed entry both pass the normal task-map pressure flow:
19 loopback requests, 195 history messages, 4 completed tasks, matching observed/saved
66,262 synthetic graph tokens under 100,000, two fresh visible pending updates,
explicit retry and active filtering with its window closed. Final planner budget
guidance appears in outgoing requests. Independent review inspected the captures.

Install job 57166 (`scratch/FLT-336/install.log`) staged and atomically replaced
`/Users/aji/.local/bin/ca`, signed the installed destination and verified matching
candidate/staged/installed hashes, mode 0755, uid/gid 501/20, version/help and valid
signature. Recovery preserves signed 0.1.87 at
`/Users/aji/.local/bin/.ca-release-stage.btaFwy/ca.recovery-ORC258-20261108T214206Z-db7f9b5bc70ffb4a2ca4a4654c18cd18f90acbc4e9d3db5d35b9fabbfee3e2bf`.

Final preservation matches all 4,457 protected runtime metadata entries and all 56
original saved evidence hashes. Only the authorized installed binary differs.
All frozen source hashes still match. No implementation or delivery work remains;
bounded real-provider continuation remains unqualified as described above. No
further provider calls are queued. See `scratch/FLT-336/final.json`,
`installed-artifacts.json` and `review-gate.json` for receipts.
