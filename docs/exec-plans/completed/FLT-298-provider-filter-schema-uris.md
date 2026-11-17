# FLT-298: preserve tool schemas while hiding content URLs

Status: complete and installed as signed ca 0.1.41 through the coordinated ONE
Mac replacement. The installed minimal DeepSeek prompt succeeds with the filter on.

## Acceptance and scope

Before repair, the normal installed `ca` (0.1.40), with the user's enabled all-URL hiding,
Latin page stand-ins and image placeholders, rejected even a new minimal
DeepSeek prompt with HTTP 400. The rejection identifies the `read_file` tool
schema's rewritten dialect URI as invalid. A fresh in-memory headless run
reproduced this without fetching pages, reading links or changing saved sessions.

Preserve schema validation values and references during URL hiding; continue
hiding URLs in conversation content and schema descriptions/titles. Keep the
standalone page transformation unchanged. No unrelated provider, verifier or
pending CBX-297 source changes belong to this repair.

## Progress

- [x] Inspect installed entry point, source boundary and enabled filter settings.
- [x] Reproduce the exact HTTP 400 with a minimal fresh installed run.
- [x] Demonstrate the failure with a built-in tool schema regression.
- [x] Repair request URL transformation and obtain independent review.
- [x] Run focused checks and the required frozen-source completion gate.
- [x] Build, recoverably install, and read back the signed release; retry the
      same minimal DeepSeek acceptance case through installed ca.

## Evidence and constraints

Evidence lives under `scratch/FLT-298/`; original source and document copies
are retained in `before/`. During the original repair the user forbade visiting
or reading URL contents, while authorizing ca to reproduce the provider error.
The subsequent follow-up explicitly authorized benign live page fetches; that
separate verification and its limits are recorded below.
The repository has no `.git` or `.agent/PLANS.md` in this checkout. Current
source version was already 0.1.41 for pending CBX-297; installed version began at 0.1.40.
Preserve that task's existing changes and record the final artifact identity.

## Diagnosis

`prepare_request` applies schema-aware word adjustments, then its separate
`hide_all_urls` pass processes the entire serialized JSON as raw text.
That pass replaces the schema authority and Latin-encodes the dialect path.
The provider rejects the malformed schema before answering. Local filter
configuration loads successfully; this is request corruption, not a filter trip.

## Validation

The failure-first built-in schema regression failed before repair. All 62 focused
filter cases pass after repair. Independent combined review accepts filter SHA-256
`5d922ac6e73af6211c5d738fb0b0d82b390ab6eb3d14fdd239aaa77d9e49cd92`.
The repaired debug candidate, using the same default configuration and filter,
returns `success: true`, final message `OK.` from DeepSeek for a fresh minimal
in-memory prompt (`repaired-deepseek.json`). No tool or page fetching occurs.

Unchanged full gate attempts use immutable copies with disposable application state,
offline dependencies and the existing shared compiler cache. Original 145 generated
`.snap.new` outputs are retained byte-for-byte in root and omitted only from the
verification copy; accepted snapshots and all criteria remain. Historical passing
baseline is `scratch/CBX-297/gate-v5/`; its later-discovered criteria incompatibility
is recorded below. Root frozen inventory digest is
`02f8619878975b2ce43816afa828d1cb0ea1d010d6a61b16a649ddc996ba096c`;
gate copy digest is `69d36efaca22361e14d0281afe7b4f47e483a79dd93c51fc11917e289c01a3d1`.
V1 passed preflight, verifier tests, format and strict lint but its main default
matrix had 3445 passed / 1 failed / 42 ignored. The launcher's exported compiler
target override leaked into a nested sandboxed Cargo fixture, correctly denied
writing the shared target lock. The exact unchanged fixture and same compiled
harness pass after removing only that launcher override. V1 was interrupted
during the remaining workspace targets and remains infrastructure-failed.
V2 was stopped before tests after detecting that the historical baseline's
runtime-verifier criteria differ in the two already-present CBX-297 verifier
files; comparing them would be invalid. Both interrupted attempts are retained.

The independently reviewed scratch launcher now reuses target outputs through
a source-copy symlink without exporting a target override into tests. V3 runs
the unchanged full gate and standalone evaluator without the optional
incompatible baseline comparison. All stages, criteria, assertions and ignored
tests remain intact; no performance comparison is claimed. V3 preflight,
verification contracts, formatting and strict lint pass. Its complete default
workspace matrix passes 4328 / 0 / 43 across 29 targets; all features passes
4484 / 0 / 44 across 30 targets. No tests are filtered and prior ignored
identities remain unchanged. The release build, all nine native terminal cases
and independent evaluator pass; the complete gate is **passed**. Source before
and after remains exactly `69d36efaca22361e14d0281afe7b4f47e483a79dd93c51fc11917e289c01a3d1`.
The signed artifact is `scratch/FLT-298/gate-v3/candidate/ca`, version 0.1.41,
SHA-256 `71fb5770bec9101ffbae59d4a7e941f797723f38d5a03ecfdd0d0c6a1df2fadc`.
Its hash/version and valid-on-disk signature readback pass. This exact artifact
was delivered to and installed by the coordinated release owner.
Receipt index: `scratch/FLT-298/final-gate-assessment.json`; complete installed
proof is recorded below and in `scratch/FLT-298/final-assessment.json`.

Final independent reconciliation confirms every stage log/candidate hash, all
nine terminal cases and all 145 original snapshot outputs. The coordinated
FreeBSD task subsequently changed only `crates/ca-automation/src/mcp/server.rs`
in live root for its separately reviewed `cfg(test)` shell fixture portability.
That later delta is outside this frozen Mac source and gate. Mac installation
uses the exact qualified artifact above; native/publication evidence binds the
later delta separately. No claim that v3 tested the later live-root digest is made.

## Changed files

`src/provider_filter.rs`, `docs/CONFIGURATION.md`, `CONTINUITY.md`,
`NEXT_STEPS.md`, and this plan. Scratch-only evidence/launcher files do not
change the product or verification criteria.

The user authorized coordination with the existing CBX-297 chat. That chat owns
the ONE final signed 0.1.41 installation and publication, including this exact fix.
This task did not independently bump, replace binaries or modify the verifier.
The release owner performed exactly one installation of the qualified artifact;
the installed executable now reports 0.1.41. Earlier candidate-only evidence
remains distinct from the complete final gate and installed proof below.

## Installation and actual entry-point proof

`/Users/aji/.local/bin/ca` reports 0.1.41 and has exactly the qualified signed
candidate SHA-256 above, mode 0755 and uid/gid 501:20. Candidate/staged/installed
identity, valid-on-disk signature, version/help and exactly one replacement pass
in `scratch/CBX-297/final-macos-install-filter/installed-readback.json`.
The prior 0.1.40 recovery copy's SHA-256 was independently checked as
`4660c3687ff50f4f8a2d1f0f05673a5cab8c780246b31c756ebf556b024eda9a`.

One actual normal-PATH installed run with `--no-session --single-turn
--exclude-workspace-context --output-format json`, prompt `Reply with OK. Do not
use any tools.`, returns success true, final message `OK.`, exit 0. Stderr
confirms the original enabled URL hiding, Latin pages and image placeholders.
No tools/pages/links were opened. Receipts: `installed-deepseek.json`,
`installed-deepseek.stderr` and `final-assessment.json` under `scratch/FLT-298/`.
Running user sessions were left alone; restart ca to load the new executable.
No FLT-298 work remains. Separate FreeBSD/publication closeout belongs to CBX-297.

## Authorized live page-filter follow-up

The user subsequently requested real benign page fetches to verify that the
installed filter still hides URLs and page content from DeepSeek. The same
installed artifact above ran two fresh in-memory headless native `web_fetch`
workflows with the existing enabled filter. Example Domain and IANA's Example
Domains page fetched successfully; Wikipedia's Cat page returned HTTP 403.
All four DeepSeek requests returned HTTP 200 and both runs completed `DONE`,
exit 0. No source, configuration, credentials or saved sessions were changed.

The actual post-filter request-body traces show opaque `ca-rNN.invalid` URLs
in the prompts and tool calls, with no original target hostnames. IANA's four
embedded absolute ICANN links are masked too. The matching fetched-page tool
messages contain Latin stand-ins and none of the original visible prose words.
Ordinary trace sanitization did not redact or omit these target messages; source
inspection confirms this is the same filtered body attached to the HTTP request.
Independent review accepts both traces and their exact hashes.

Coverage is the forwarded tool excerpts, bounded to 4 KiB before substitution;
IANA's local result already omitted 2710 source bytes. HTML structure, IDs and
classes remain visible, including IANA's `example-domains` ID. JSON Schema
protocol identifiers remain exact. Wikipedia proves only URL masking because
its article was not obtained. Evidence: `scratch/FLT-298/live-pages/assessment.json`,
the two run JSON files, and `trace.jsonl` / `iana-trace.jsonl`. The initially empty
trace directory received only these two owned traces; prior traces were not pruned.
