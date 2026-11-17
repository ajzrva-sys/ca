Do not start this plan unless the task names it. The continuation is [AGT-271](AGT-271-task-first-economic-delegation.md).

# Provider timeout and continuation recovery

## User request and scope

Fix the timeout/transport failure shown in the supplied October 11 screenshots for `/Volumes/projects/ca`. Do not read previous ca session content. Investigate source and deterministic loopback fixtures only. Preserve existing dirty changes, settings, sessions, running processes, and credentials.

## Acceptance

- Live nonempty stream bytes, including Responses lifecycle/keepalive frames, prevent false inactivity timeouts.
- Retry suppression preserves configured deadlines.
- HTTP error diagnostic reads are size/time bounded and cancellable; error excerpts are credential/terminal/Unicode safe.
- Protocol-terminal completion cannot be turned into a timeout merely because HTTP EOF is delayed; invalid/incomplete responses still fail.
- Continuation retry preserves completed tool results and the original request bytes/owner. Failure records retain the prepared provider/model and actual error category.
- Focused regressions, full verifier, signed release, and recoverable installed artifact are checked with real outputs; unavailable live-provider evidence is reported as unavailable.

## Current evidence

Evidence root: `/Users/aji/.hermes/cache/scratch/ca-provider-recovery-20261011/`.
Source-only backups in that directory preserve the dirty starting versions. No runtime sessions or prior conversations were inspected. The starting checkout already contained body-byte liveness and first-token timeout changes.

- `model-red.log`: new regression reproduces `mimo/unknown`; `model-green.log`: one test passes after retaining last prepared route identifiers.
- `route-reset-red.log`: stale route survives new turn; `route-reset-green.log`: four recovery tests pass after clearing it on begin/cancel.
- `category-red.log`: first-token timeout was recorded as `transport failed`; `continuation-green.log`: 15 continuation checks pass after forwarding the typed error to scheduling, including request/tool/owner preservation.
- Independent source investigation identified timeout-policy reset, unbounded error-body extraction, unsafe excerpt truncation/redaction, and possible completed-stream/EOF stall. Transport implementation and fresh failure-first evidence are in progress.

## Implementation boundaries

Parent edits `src/main.rs` and `src/turn_recovery.rs`. Transport implementation edits only `src/agent/client.rs` and `src/agent/client_tests.rs`. No broad formatter, existing test weakening, git reset/stash/commit, or runtime configuration changes.

Another process changed unrelated source while focused checks ran. Full verification must bind a stable candidate source and review only this task's incremental diff. Baseline backups are source evidence, not conversation history.

## Remaining work

1. Finish and read back transport implementation and retained RED/GREEN results.
2. Independent code review; fix only confirmed findings.
3. Run repository verifier with disposable state and scratch-only temporary paths. Read report/evaluation/failing logs, retain failures.
4. Build/sign release and stage recoverable atomic installation under standing repository release instructions. Verify exact candidate/installed identity, signature, version/help. Do not claim a live MiMo outage or a live provider fix without evidence.

The repository's `.agent/PLANS.md` referenced by AGENTS.md is absent. This self-contained plan records requirements and evidence without inventing the missing template or reading historical work summaries.
