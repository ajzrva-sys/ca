# BUG-341 — Real DeepSeek filter listing

Status: COMPLETE — signed CA 0.1.99 installed and real-provider acceptance passed. User explicitly requests the exact normal-CA task
`testing filter. list ~/projects` with real DeepSeek and repair of any failure.

## Acceptance and boundaries

Launch installed ca 0.1.98 from `/Volumes/projects` with the saved DeepSeek
configuration, ordinary tools/permissions and workspace context. Use a disposable
CA home/session and native authentication; credentials are copied opaquely for
CA itself and are never inspected or logged. Preserve production settings,
sessions and running processes. Source writes are denied during this listing test.
No loopback model, prescribed model answer, tool whitelist or bypassed approval.

Verify local preview before transmission, one Send, actual HTTPS DeepSeek
responses, automatic planning/execution, correct directory listing and usable
local path restoration. Inspect `/filter` after the accepted task. Keep actual
failures and distinguish pending work from failure. Repair only observed blockers.

If source repair is necessary, freeze changes, obtain independent review, run
focused checks and the applicable project gate, then build/install a signed
recoverable release and repeat installed real-provider acceptance. No release
is necessary if current behavior passes. Evidence: `scratch/BUG-341/`.

## Progress

- [x] Installed baseline is ca 0.1.98, SHA-256
  `1876595a1750676b08aa8bedb821a762beb0c2e354d6c63052d8b3c7857177b6`.
- [x] Native offline readiness from copied settings confirms saved credential
  reference and default `deepseek/deepseek-v4-pro`; no provider request yet.
- [x] Real normal-entry observation, diagnosis and corrected debug completion.
- [x] Required repairs, checks and recoverable signed installation.
- [x] Independent real-provider evidence review and completion records.

The historical `.agent/PLANS.md` path is absent; this follows the existing
completed-plan structure. This is a user-requested CA extension, not parity work.

## First real-provider observation

`real-installed-baseline/receipt.json` records a 680 ms preview, zero traffic
before Send, one Send and a real DeepSeek-v4-pro HTTPS HTTP200. The saved author
workflow then fails with `Unknown filename reference; request was withheld`.
No native tool success or listing occurred. The process exits cleanly.

Independent inspection of the first actual outgoing request finds 81 substitutions
inside static CA-authored instructions (37 distinct tokens), including task,
graph, tools, evidence and the required `## Task:` heading. The exact user path
`~/projects` remains unbound to a protected executable reference. Both are confirmed
integration defects. They are not proof of which generated reference caused the
failure: the native trace reached its16MiB cap during reasoning before recording
public output, and saved child state retains no model result. Do not weaken the
unknown-reference guard or claim the missing token was recovered.

Repair ownership: automatic_execution handles closed runtime-only host-instruction
provenance through construction/preparation/final wire validation; runtime reviewer
handles existing tilde/symlink paths within current authorized roots in filenames.
Dynamic task/project/skill/history/result text and explicit stop rules stay filtered.
Root owns native qualification, combined review/checks, release/install and docs.
A complete0.1.98 source baseline is retained in `scratch/BUG-341/baseline-0.1.98/`.

- Filename implementation is frozen for review. It binds existing home-relative
  symlink paths only within currently authorized roots, retains exact local text,
  and rechecks targets on execution. Root also added typed automatic-map path
  normalization: provider references become workspace-relative grants/evidence
  locations while prose keeps its authored spelling. No access is broadened and
  unknown references still fail. Host-instruction provenance is in progress.

- Combined source was independently reviewed with no boundary blocker. First
  focused build `scratch/check-change/runs/20261109T233251Z-53746` failed to
  compile: recursive authored-text projection added a reference to its generic
  callback on each instantiation. Replace the recursive implementation with a
  fixed callback type; do not raise the recursion limit. No release was built
  or installed. The failed receipt remains retained.

- Second focused run `scratch/check-change/runs/20261109T233447Z-58077`
  compiled and passed132 cases, including the new host-provenance and home-path
  tests. One new typed-graph fixture failed because it opened a context without
  enabling filename filtering. Enable the same test-local filename policy used
  by existing privacy tests; production code is unchanged by this correction.
- Package version advanced once to0.1.99 for the intended corrected release.
  Installed binary remains0.1.98. Final focused checks and full gate are pending.

- Focused run `scratch/check-change/runs/20261109T233706Z-63392` passed133
  tests, formatting and strict all-feature bin lint. Independent review remains
  clear. A final narrow instruction correction names existing authoritative
  `tool_success` evidence for native inspection, with a native-map preflight
  regression for list_dir('.') and rejection when its tool grant is absent.
  This is an established guidance/contract mismatch, not changed permissions.
  The full gate will cover this final wording/test delta and build0.1.99 once.

- First full gate `scratch/BUG-341/full-gate` passed preflight, workflow contracts
  and formatting, then rejected one test-only field-reassignment initializer in
  strict all-target lint. Root stopped the gate before any release build; its
  partially compiling default matrix and evaluation are explicitly nonpassing
  (interrupted), not reused as passes. Use the equivalent struct initializer
  with automatic_task_map:true. Source is frozen again for the corrected full
  gate; package version remains0.1.99 and no new release is installed.

- The corrected debug binary was tested through normal CA against real DeepSeek
  while the full gate ran. It still failed before its second HTTP request. First
  request independent inspection confirms all closed host instructions and the
  home-root binding are now correct; public calls are list_dir(".") and
  glob("**/*"). The failure is in continuation preparation. Evidence:
  `scratch/BUG-341/real-debug/`. No release or install occurred.
- `full-gate-final` passed strict workspace/all-target lint and the complete
  default matrix (982.847s), then was deliberately interrupted at the beginning
  of all-feature testing because real-provider acceptance failed. It is not a
  passing full gate. Preserve its evidence; diagnose the continuation before
  another final release build.

- Continuation diagnosis is now concrete: first response completed with two
  valid native calls; preparing the second request validated provider-owned
  ChatReasoning as ordinary filename text. Metadata-only inspection found five
  unissued alias-like strings in that continuation. Exact route/policy-owned
  continuation now receives an exact concrete-field receipt, retaining its bytes
  and whole-body stop checks. Visible text and tool arguments remain checked.
  The independent three-file review found no blockers.
- New reproduction first failed to compile due to one missing test Arc
  qualification (`20261110T000917Z-94310`), then reproduced the exact unknown
  filename error before the production fix (`20261110T001036Z-97239`).
  Both failed receipts are retained. The corrected focused run is in progress
  at `scratch/check-change/runs/20261110T001308Z-3150`.

- The continuation correction passed focused checks, formatting and strict lint
  (`20261110T001308Z-3150`). Debug build identity
  `1e21ed940f8baaf4384b20e51c949c1d108c23f6a8fe5b53ef363a2bef312624`
  then reached two real DeepSeek HTTP200 responses, proving that repair. The next
  failure is distinct and retained in `real-debug-continuation`: all15 listing
  calls finish (9 success,1 ordinary failure,5 denied), then preparing the next
  request mistakes an unprefixed denial sentence for a filename. Both diagnosis
  agents independently found the same seam: list_dir projection ignores existing
  `tool_result_success=false` and adds the diagnostic marker only later. Honor
  that existing status before row parsing, retaining ordinary diagnostic privacy.
  No permission or model-answer tailoring is involved. No release built/installed.

- Scope+safe-diagnostic checks passed at `20261110T002833Z-37530`. The next
  real run `real-debug-scope` reaches8 HTTP200 responses, but public search_files
  calls search for source_1 and its NAME token instead of opening the provided
  home target; it ultimately encounters unknown/traversing references in searched
  data. Generic scope wording alone did not solve target recognition. Existing
  home paths will use already-supported PATH-shaped spelling references, retaining
  exact original restoration, historical aliases and current canonical authority.
- The separate prior immutable-field failure is undergoing a deterministic local
  reconstruction from the saved request and four native source receipts. No
  unrelated source data is being sent in a new provider diagnosis task.

- Typed-home checks passed `20261110T003658Z-56197`. Real run `real-debug-home`
  authors a valid one-task map, starts it automatically, and successfully lists
  both the workspace and exact home reference. The model then tries stripped and
  slash-suffixed variants. Native execution denies them, but historical listing
  root construction resolves the failed call again and aborts continuation. Skip
  root construction for call IDs with explicit failed-result status; retain
  argument/diagnostic privacy and native denial. Generic guidance will clarify
  that opaque references are identifiers, not names to search for.
- Offline URL reduction in `offline-request6/` isolates the other rejection to a
  URL alone. Task masking issues a ca-rNN.invalid authority, which collides with
  an existing private basename `invalid`; a second filename pass rewrites it.
  Earlier read-footer/receipt explanations were incomplete hypotheses: no footer
  is needed. Preserve only authority spans actually issued by the accepted task
  URL map, keeping surrounding/path/query text checked; no whole-read exemption.
  Root remains uninstalled0.1.99; installed0.1.98 unchanged.

- URL + failed-call corrections are frozen and independently clear. Focused run
  `20261110T004812Z-81078` passed31 cases; the URL case passed actual native-read
  final validation/restoration but failed an assertion that every unissued public
  authority must be changed. That is not an existing policy. Test-only correction
  now checks no borrowed authority spans and equality to ordinary masking; actual
  private path/query, unknown filename and stop checks remain. Rerun pending.

- URL fixture passed final checks at `20261110T005314Z-92238`; prior31 unchanged
  cases passed. The intermediate second test failure (`20261110T005102Z-87310`)
  assumed unselected JSON data is private; documented default permits it. The
  corrected test uses supported HTML data, asserts standalone masking first,
  then retains the private URL-tail assertion. Production was unchanged.
- Real `real-debug-integration` confirms four successful listings/reconfirmations,
  then fails after glob receives the complete HOME reference as a literal pattern.
  Source does not rewrite it to **/*, but projected glob allocates aliases for
  every visited file before matching; a literal directory can match no files
  while expanding up to50k aliases. The next registry snapshot hits its existing
  8MiB value limit. Implement an exact-issued literal fast path (files-only semantics,
  current scope/base validation, no unrelated traversal), preserving wildcard rules.
- Duplicate-listing presentation only says duplicate/reconfirmed, omitting successful
  requested-ref/canonical-directory equivalence. Preserve authenticated observation
  and retained basis and explain that equivalence in the native provider note.
  This is a concrete result-context gap, not a claim to know model reasoning.

- Literal glob + authenticated duplicate clarification passed combined focused
  checks, formatting and strict lint at `20261110T010825Z-26038`; independent
  combined review found no blockers. Root is building a copied debug executable
  for live retest. No release build or installation has occurred.

- Exact root task now COMPLETES with real DeepSeek-v4-pro in
  `real-debug-reference`: zero pre-Send traffic, one Send, six HTTP200 requests,
  completed saved plan/graph, correct visible 14-entry listing at ~/projects,
  /filter wide+narrow with no additional request, no control intervention and
  clean terminal restoration. Completion took133.466s; local preview1.194s.
  Debug artifact4b479d5462b7aa3397a36459825aac81bf7bd5ade7327d268afa4fb5e27f99e6
  matches reviewed source6b0e970677808b5f441330b7c63f43ed90868939638ab82bf72511bd42f309cf.
  Independent real evidence audit is pending, separate from the completed combined
  source review. Full gate `full-gate-qualified` is running (job44647), and the
  same debug artifact is testing ~/projects/filtertest (job17240). Installation
  remains0.1.98; no release0.1.99 has been built yet.

- Both final debug real-provider runs passed: `real-debug-reference` and
  `real-debug-filtertest`; each has six HTTP200 responses, one Send, zero
  pre-Send traffic, automatic completion and independent native audit.
  Full gate `full-gate-qualified` caught a test-only single-range-array lint.
  It was stopped during default compilation before any release; evidence is
  retained as nonpassing. Equivalent length+range assertions fix the lint.
  Production source and both native debug qualifications are unchanged.

- Final gate `full-gate-release` passed formatting, strict workspace/all-target
  all-feature lint, complete default tests (1089.952s) and complete all-feature
  tests (906.470s). Its single intended0.1.99 release build is now running.
  Final reviewed source: `3c17faab13c5bd61e42cf09681853b271fcfe99b5d5c2fe8cf2386cd3b29802d`.
  Source/build success is not yet installation or final native release proof.

## Final delivery

The complete final gate passed: 126 Python workflow tests, 4,976 default Rust tests,
5,132 all-feature Rust tests, formatting, strict all-target lint and 9 runtime
cases. 45/46 opt-in Rust tests remain skipped and documented. Gate and evaluator:
`scratch/BUG-341/full-gate-release/`; independent audit:
`scratch/BUG-341/full-gate-stage-review.json`. All final source/criteria files
match the reviewed frozen tree (`source-final-readback.json`).

One release was built, signed and installed as 0.1.99 with SHA-256
`bae74cc8d904bdf07784a06ccd820bb112639dedad39165408f635e9419bd128`.
Installation job 2987 and complete signing/hash/mode/ownership/version/help checks:
`scratch/BUG-341/install.log`, `installation.json`. The exact prior 0.1.98 binary
is preserved under `.ca-release-stage.7NmbDL`; its full recovery path/hash are in
the installation receipt. Running sessions were left alone.

The signed candidate's exact child-folder task completed in 63.987s (five real
DeepSeek HTTP200 requests); the installed exact root task completed in 120.626s
(eight HTTP200 requests). Both used one Send, no traffic before Send, normal
permissions, no manual controls and automatic saved execution. `/filter` remained
local and readable in wide/narrow layouts. Independent native audits pass in
`real-candidate-filtertest/` and `real-installed-final/`; complete traces remain
below their 16 MiB cap. The final root contains 15 entries, including the newly
present `torcrawl`; its current authoritative listing matches the restored result.
The child has 2 entries. The installed worker recovered from one correctly denied
incomplete alias using its existing narrow listing grant; no guard was relaxed.

Settings, credentials and saved-session metadata remain unchanged. Eight existing
background-state metadata entries changed and one background entry was added;
these are recorded without claiming the entire runtime tree was unchanged.
Native tests deny production-state/workspace writes and use disposable CA homes.
Temporary credential copies from closed native runs were removed. Evidence:
`scratch/BUG-341/protected-readback.json`. No production saved session, cross-platform
run, universal secret coverage or general provider-effectiveness claim is made.

Final qualification: `scratch/BUG-341/qualification.json`. No remaining BUG-341
work is queued. Earlier failed attempts above remain retained as nonpassing.
