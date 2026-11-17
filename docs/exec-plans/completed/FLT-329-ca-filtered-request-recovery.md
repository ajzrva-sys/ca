# FLT-329 — preserve filtered CA context and identify policy failures

Status: COMPLETE AND INSTALLED as CA 0.1.84 (2026-11-07 local). User redirected work from Desktop integration and clarified the requested focus is cyber blockers in CA using the new filter. Desktop work is stopped and owned runtime cleanup verified. This is a bounded CA implementation repair, not a new UX redesign or a provider-safeguard evasion project.

## Observed defects and acceptance

Normal entry point: CA's task/filter flow and manual `/retry` after a provider failure, including explicit complete-native-history import (`--conversation-history-file`). At the start, signed installed CA was 0.1.83, source baseline 1b79e815…; full passing FLT-322 baseline remains valid for unchanged targets.

Two concrete source defects:

1. OpenAI Responses `response.failed` and `error` discard structured `cyber_policy` and return generic ResponseFailed. Recovery labels that as incomplete response instead of the existing nonretryable refusal category. Recognize only the known exact structured code, preserve generic handling of unknown/malformed errors and never echo arbitrary upstream body text.
2. Manual retry creates a new local session UUID but leaves imported NativeHistory bound only to the old UUID. The next request silently omits the full imported prefix. Bind the exact same immutable history to an explicitly validated intentional retry child, preserve route/model/protocol and unrelated-session isolation, and fail closed without losing the original binding if local preparation/save fails.

Existing filtering does reach mutable imported history at final submission while preserving schemas, IDs and opaque data. No actual CA cyber refusal was established in earlier tests; historical real block belonged to Desktop. These fixes do not force upstream acceptance and must not be represented as a reproduced block-to-filtered-success result. Provider-issued immutable replay arrays remain governed by existing receipt/privacy/stop-scan rules; no security context is removed to obtain acceptance.

## Ownership and allowed changes

/root/full_session_reconstruction owns request-store intentional retry inheritance and its main integration/tests. /root/same_client_replay owns Responses decoder and existing refusal-category regression tests. /root/replay_controls independently reviews combined scope/privacy/acceptance. Parent owns sequential checks, final integration, notes and authorized build/install. No parallel Cargo jobs or shared target lock races.

Allowed product files: src/request.rs, retry-related portions of src/main.rs, src/agent/provider.rs, tests in src/turn_recovery.rs (src/agent/stream.rs only if proven necessary; existing ResponseRefused should suffice). No session/request wire format change, approval weakening, provider fallback, cyber keyword evasion, source-project trust receipt forgery or unrelated cleanup. Live ESXi and production config/sessions stay unchanged. Applicable version files and project notes change only for the authorized release.

## Work and validation

- [x] Preserve exact original source files and baseline under scratch/FLT-329/before, before.json and protected-before.json. Baseline digest matches prior qualified source, 1,217 files; installed hash cd3d41fe….
- [x] Add test-only regressions and independently review them before implementation. No real provider/credential/ESXi payload is used.
- [x] Run frozen tests against original behavior and retain actual assertion failures. Red run 20261108T002751Z-63585 compiled: two pass, three fail; known policy code returned ResponseFailed instead of ResponseRefused, and retry outbound input had one current task item rather than imported prefix plus task. No compilation/fixture error is called the reproduction.
- [x] Implement the two narrow defects with no unrelated behavior changes. Exact cyber_policy on failed/error events uses existing nonretryable ResponseRefused. Intentional retry uses validated one-use same-Arc inheritance, successful branch save before commit, source/destination recheck and rollback of only the new mapping if applying context fails.
- [x] Freeze combined source; independent review and focused regression/privacy/retry/isolation checks, formatting and strict Clippy for affected targets. Green report 20261108T003455Z-92335 passes all 29 selected tests, formatting and strict production Clippy; separate strict bin+test-target Clippy passes (99.259s, source unchanged).
- [x] Verify through normal CA entry using disposable local provider fixtures: structured refusal correctly reported, no automatic unchanged-policy retry/tool execution; intentional retry preserves full prefix/tool pairs/opaque data and applies selected benign-label filters; unrelated chats remain unseeded.
- [x] After applicable validation, bump patch once, build one release artifact asynchronously, recoverably sign/install via project procedure, read back actual version/help/hash/signature and the repaired normal entry.
- [x] Record failures/limitations, protected-state readback and actual install outcome; no claim that upstream cyber refusal is bypassed.

Tests staged: structured nested/top-level policy failures, unknown/malformed/case/prefix/message negatives, partial tool fragments cannot complete a turn, refusal remains nonretryable, MainTurn and resolved-tool `/retry` retain native prefix and reapply current benign filters. Existing unrelated-conversation tests remain intact.

Use focused check-change workflow for this internal bounded repair if independent combined review confirms no format/shared approval changes. Broaden only if actual final scope warrants it. No redundant whole-workspace matrices or release builds. Test application state is disposable; no API keys read/printed/copied.

At ten minutes reassess what works, what is installed and what remains necessary; cut optional scope and continue the authorized repair.

## Current evidence

Frozen product identities: request3a410825…, maina84f8710…, providerd940436c…, recoverydf83af8f…; stream/native-history source unchanged. Independent combined review verifies no session/request serialization, approval, route/fallback or filter-rewriting change. Existing unrelated-session isolation and native provider/protocol/model/privacy/provenance checks stay intact. Focused gate is appropriate for this bounded internal runtime repair; no redundant full workspace matrix.

Tests cover persistent/ephemeral main-turn and resolved-tool-continuation retry, exact original/canonical messages and same imported Arc, opaque/tool-call preservation and current benign filter, unrelated UUIDs, mismatched ownership/provider/model/workspaces, stale source/destination, and local save failure before binding. Partial-tool unit test proves no completed assembler turn; actual no-execution/no-auto-POST evidence passes in the normal-entry fixture.

The real terminal acceptance passed on the exact signed release candidate (normal-entry/FINAL.SAFE.json and actual-candidate-v5/receipt.SAFE.json): synthetic structured policy refusal, no arbitrary error echo, no automatic second request, no partial tool execution, then normal `/filter on` and manual `/retry`. Both captured bodies contain all four imported prefix items; opaque data, completed tool IDs and all 54 tool schemas remain exact. The second body applies the selected harmless PrivateCanary label alias and restores the task exactly once. Two local requests, clean exit, restored terminal and closed owned listener. This proves plumbing, not a real upstream refusal or refusal reduction.

Four fixture failures remain preserved: incompatible CLI flags (zero POST), a test guard denying ordinary workspace-coordinator metadata (zero POST), intentionally unreceipted source-project privacy rejection (one POST), and missing local final-response request-ID metadata (two POST). Only fixture corrections followed; product source and release artifact stayed frozen. A nonessential summary-print IndexError after saved passing receipts is retained separately. Ordinary native coordinator registration and one idle-descriptor prune occurred; 16,709 preexisting stable locks retained exact metadata, owned locks are unheld, and no manual registry mutation occurred. There were no active owners in the baseline, so no active-owner preservation claim is made.

Validation passes are preserved in scratch/FLT-329/red-check.log, green-check.log and strict-tests-v1/report.json. Baseline red (2pass/3fail) remains unchanged. Combined independent review f2136aff… and later green log/source verification confirm the bounded repair; no full workspace matrix is claimed.

Root advanced 0.1.83 to 0.1.84 once. Background release session 83219 completed successfully in 336.737s, recorded in scratch/FLT-329/release-v1/report.json and release-job.log. The single all-features release is SHA-256 `0b202921215a3c8b6c5f1adbeef93afd854ba3075b84713703faf37c75f31fb9`; independent source/artifact and actual terminal result reviews pass. No additional workspace matrix or release build was run.

Installation session 78244 completed successfully using tools/bug257_install.sh; preflight/install logs are scratch/FLT-329/install-preflight.log and install.log. Installed `/Users/aji/.local/bin/ca` is the exact tested artifact, mode 0755, uid 501, valid ad-hoc signature, version/help readback pass. Private installed `ca filter check --text 'PrivateCanary note' --hide PrivateCanary` passes. Exact-artifact terminal evidence is reused; the prepared duplicate installed two-request scenario remains explicitly UNRUN.

Recovery copy: `/Users/aji/.local/bin/.ca-release-stage.fcVgEX/ca.recovery-ORC258-20261108T010304Z-cd3d41fe04f00c2cb5c36314c29d4cc72e8195eb5d1dcff943088ad8ab568fb9`, original hash verified. Installed readback receipt: scratch/FLT-329/installed-readback.SAFE.json. All 4,457 protected CA config/session metadata entries match baseline. All 1,218 release source files match their built identity; oracle/reference unchanged. Live ESXi was not accessed or changed by this repair. Running user sessions were left alone.

The initial final-readback wrapper used the baseline file-count field as a map and failed after successful entry checks; its failure is retained in the corrected receipt, with no product change. All required bounded-repair checks and installation are now complete. Genuine upstream cyber-refusal reduction remains unverified; no such success or guaranteed acceptance is claimed. No further Desktop or model experiments are queued.
