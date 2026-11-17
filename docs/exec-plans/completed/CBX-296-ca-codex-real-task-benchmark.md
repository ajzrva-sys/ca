# CBX-296: ca versus Codex on real coding tasks — complete

## Result

Installed ca 0.1.40 completed 0/4 tasks; installed Codex CLI 0.162.1 completed 4/4. Both used GPT-6.1-Sol with high reasoning, identical prompts and fresh parent checkouts on this host. Correctness required an implementation edit and both independently restored original and accepted held-out regression suites passing. No rescue prompts or operator timeouts occurred.

Codex successful task times: 91.989, 52.377, 63.920 and 100.893 seconds; median 77.954 seconds, total 309.180 seconds. All 45 local commands were independently reviewed: no external solution fetches, requests for help or artificial sandbox denials. CA made no implementation edits: three native Responses streams rejected `response.metadata`; the platformdirs prompt rejected literal `$HOME-prefixed` as an unknown skill before a request. CA's 18.367 seconds to exit across four failures is not successful task completion time.

Codex usage: 1,094,347 input tokens, including 952,192 cached, and 12,776 output tokens, including 3,522 reasoning tokens. Short-context Standard API-equivalent estimate is $0.507289 across all four scored runs. Conservative model-pricing upper bound is $0.777832 because aggregate turn usage does not expose each physical request's context length. Actual subscription invoice attribution is unavailable. CA's failed streams lack terminal usage; comparative completion cost is unmeasured. Published rate basis: https://developers.openai.com/api/docs/models/gpt-6.1-sol.

## Checklist

- [x] Pin four real historical bug fixes across three independent public Python repositories before agent outcomes.
- [x] Preserve parent archives, prompts, accepted test-only snapshots and their hashes; keep accepted implementations and hidden tests outside agent access.
- [x] Qualify failing parent regressions, passing accepted fixes, original suites and accepted implementations against restored original suites.
- [x] Qualify each original full host-compatible suite inside its actual per-run sandbox before starting the timed model call.
- [x] Freeze model, reasoning, executable hashes, input hashes, source/evaluator identities, paired order, 300-second cap and scoring before clean runs.
- [x] Execute all eight clean runs without rescue prompts.
- [x] Independently evaluate original and hidden suites; preserve all failures.
- [x] Independently review all outcomes, log hashes, commands, usage and cost arithmetic; accepted with no critical findings.
- [x] Archive credential-free run outputs and record unchanged executable hashes/versions.

## Tasks and scoring

Tasks: python-dotenv backslash round trips; python-dotenv empty unquoted comments; platformdirs quoted Unix user-dir backslashes; pathspec terminal directory globstar. Exact parent/fix identities, prompts, source URLs, archive hashes, hidden test hashes and commands are in `scratch/CBX-296/research/tasks.json`. Every agent starts from a parent-only Git snapshot with no accepted fix history. Evaluator copies the result, restores all original tests and root collection controls, then applies verified accepted test snapshots. Agent edits cannot weaken these tests. Both suites must pass, and implementation code must change.

Original and accepted qualification receipts are retained in research/qualification*.json, research/qualification-v2/ and research/qualification-fixed-original/. One existing dotenv GNU-printenv-only test fails on macOS on both original and accepted source; it is excluded identically. Optional IPython, platform and pathspec native backend cases remain documented skips.

Timing includes native CLI startup, model calls and local tools, and ends at process exit. Preparing the fixture and independent tests are outside timing. Each task's original test suite runs as an environment check before timing, identically for both harnesses. Native product prompts and toolsets differ; model, reasoning, task, host and autonomy agree. The final order alternates harnesses across tasks.

## State and transport

Both native model connections use direct port 443. CA uses NO_PROXY=chatgpt.com while its authentication destination stays behind an opaque CONNECT guard that denies OAuth before TLS. Codex redirects refresh to a denied loopback endpoint. Synthetic expired-auth fixtures verified both behaviors against the installed artifacts without accessing original credentials. Authentication caches are read-only; private HOME/config/session/log/database paths isolate benchmark state. The external sandbox denies research, sibling-run access and writes outside the current run except normal transient CA lock metadata and /dev/null. The shared interpreter and native package are read-only.

Proofs: netguard self-test, python-entry-qualification.json, refresh-override-proof.json and ca-native-direct-auth-proof.json. Source and installed product binaries were not changed or reinstalled for the benchmark.

## Amendments and limits

Initial plans included two repetitions. Before any clean scored outcome, scope was reduced to one run per task/harness after setup delays. No task was removed. Initial v1/v2 attempts were excluded symmetrically because concrete sandbox denials blocked DNS/direct transport, an intermediate interpreter symlink and /dev/null. The first evaluator also rejected a pristine documentation symlink; the corrected evaluator preserves exact internal baseline symlinks and isolates its own state. Every failed attempt and amendment remains preserved under invalid-isolation-pass1/, invalid-isolation-pass2/ and evaluator-amendment-v2.json. No product fix or scoring-criteria weakening was made during measurement.

This is a four-task descriptive pilot on historical public Python bugs; training contamination is possible. Other CA providers, large projects, Rust work and interactive UI behavior were not measured. Provider/input failures prevent measuring CA's coding quality, successful completion speed or comparative cost. Unassisted completion is an intervention proxy; interactive human time was not measured.

## Completion evidence

`scratch/CBX-296/final-assessment.json` indexes unchanged executable readback, frozen identities, independent review and eight archived credential-free run receipts. `summary.json` provides per-task and aggregate outcomes, timing and usage. `transcript-audit.json` preserves reviewed commands and visible messages. `evaluations/` retains result patches, source copies, exact test logs and hashes. No benchmark work remains.
