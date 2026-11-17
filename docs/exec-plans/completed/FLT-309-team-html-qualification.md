# FLT-309 — real-provider team HTML privacy qualification

Status: COMPLETE — privacy qualification passes; signed ca 0.1.67 installed.

Normal entry: installed interactive `ca --filter`, an HTML @ reference, a
read_file worker and a contained native codex-team reviewer. Both workers must
make their own real-provider requests. Audit all physical request bodies,
including prompts, native tool results and returned summaries, for synthetic
HTML data and checked encoded spellings. Verify shared aliases and the original
file, config, sessions and binary identities.

Observed installed 0.1.66: `scratch/FLT-308/team-provider-2` captured nine HTTP
200 requests (six coordinator, two read worker, one contained reviewer). No
fixture secret appears outbound. The read worker successfully reads aliased
HTML; the contained reviewer returns alias-only data. Coordinator read_file
fails: `Private file tools require an active filtered conversation`.
ApprovalBroker::prepare_formatted drops its Tokio task-local privacy context
when resolving references inside spawn_blocking. Direct workers do not use
that losing handoff. Do not label the entire workflow passing while this fails.

Small repair: capture the existing context before crossing the blocking
boundary and resolve typed fields through that exact host context. Preserve
remote IPC fallback and all actual-path workspace and permission checks.
Add a broker regression for admitted, forged, outside-workspace and denied
references. No protocol, journal, permission policy or TUI presentation changes.

Freeze source, obtain independent review, run focused privacy/approval/workspace
checks, format and strict lint. Advance the patch once and build ca once.
Install the exact signed artifact recoverably at the normal path (scratch-path
networking is already unqualified), rerun the team test and roll back on failure.
Audit summary receipts and all request fields independently. Record identities,
failures, scope limits and unchanged runtime metadata before completion.

Earlier attempt `team-provider-1` never sent a request: the fixture wrote
`@index.html.` and ca correctly blocked that nonexistent local filename.
It was stopped for inspection; its evidence remains preserved. Corrected
reference spelling is used in subsequent attempts.

Source repair is confined to `src/approval.rs`, `src/privacy/tasks.rs` and
`src/privacy/task_tests.rs`, plus the standard package patch bump to 0.1.67.
Final review/source digest:
`b0325e0fc1888a3765464c503fdeae164500ec867265add7c662cf2cd39a0a14`.
`scratch/FLT-309/independent-review.json` approves the exact source.

`scratch/check-change/runs/20261028T094604Z-84152/report.json` passes 224 focused
all-feature privacy/approval/workspace tests, formatting, strict binary Clippy
and the single intended release build. Two existing opt-in timing probes are
ignored. No broad permission/protocol/storage changes are made, and no new full
workspace matrix is claimed. The unchanged targets retain the prior FLT-307
workspace baseline. Initial `20261028T094320Z-77801` passed 223 checks; one new
assertion mistakenly expected existing canonical external reads to be denied.
Corrected test preserves the existing external-read policy. Its external write
assertion exercises the source-view guard; existing included workspace tests
separately exercise exact OutsideRoots write rejection. No evaluator/baseline
or established policy was changed.

Signed release artifact:
`d0a6df8c5be83928ed80580805b53d5d611042440e08de5adb7812b5c1a6249c`.
Two installed-path team runs exercised that same artifact. The first completed
but its audit reader assumed every private metadata JSON file was an object;
the corrected reader reads actual extensionless alias-index segments. The
second run sent 9 real provider requests with both successful file
reads, stable aliases and clean terminal exit. Every original provider-bound
privacy assertion passes. An additional assertion mistakenly rejected raw
HOST-LOCAL restored summary data, triggering another automatic rollback.
Both original failed audit/install receipts remain preserved.

`scratch/FLT-309/team-boundary-assessment.json` independently re-evaluates the
existing second installed run, retaining those failures. All 9 physical
requests have HTTP 200 responses: 6 coordinator, 2 read worker, 1 separate-trace
contained native worker. Prompts, native tool results and forwarded summaries
contain no fixture secret or checked raw/percent/base64/hex/HTML/JSON-Unicode
spelling. Four page aliases match across all roles. Native admission carries
the root privacy-context resource; its receipt succeeds. Both completed worker
summary bodies are protected by exact host-issued aliases at the coordinator
boundary; the contained model's recorded summary itself keeps page aliases.
The original HTML and fixture config are unchanged; no manual policy exists.

Privacy boundary clarification: host-local summaries and operator views may
restore original data. Those are not provider payloads. Summary bodies returned
to the coordinator currently remain opaque references, so this proves privacy,
not readable full-summary delivery. The audit explicitly records this limit.
Use the passing physical/runtime evidence for the exact artifact rather than
repeat paid provider calls due solely to the corrected audit interpretation.

Final installation: `scratch/FLT-309/install-final-receipt.json` and
`install-final.log` record exact candidate/staged/installed hash, valid-on-disk
signature, version/help, executable mode 0755 and owner 501:20. Prior 0.1.66
is preserved in the recorded recovery copy. The already-tested installed-path
artifact was reinstalled after corrected offline audit; no additional release
build or model request was needed. All 102 production config/session metadata
entries remain unchanged (`runtime-after.json`); user processes were left alone.

Stream precision: all nine attempts receive HTTP 200. Both workers complete;
eight request streams finish normally. The final coordinator stream records
consumer_cancelled when the harness exits after the displayed completion marker.
The final outgoing request body and all worker summaries are captured, so the
provider-bound privacy assertion is covered, but full drainage of that final
coordinator response is not claimed. The terminal exits 0 and restores normally.

Final source/artifact and verification aggregation:
`scratch/FLT-309/final-assessment.json`. Independent source review is in
`independent-review.json`; independent runtime boundary audit is in
`independent-team-audit.json`. Preserve original failing assertions and source
receipts; do not rewrite them as passing. Internal host summary restoration and
opaque coordinator summary delivery remain explicit scope limits.
