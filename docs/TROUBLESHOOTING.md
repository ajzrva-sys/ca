# Troubleshooting

## Build or dependency failure

Check `rustc --version` against `rust-toolchain.toml`. Install the native compiler
and platform SDK. Build with `--locked`; use `--offline` only with a populated
Cargo cache. Do not repair a local download problem by regenerating Cargo.lock.
When reporting a failure, include the failing command, compiler/platform, and a
bounded redacted error excerpt.

## macOS kills the executable

A copied or replaced binary may have an invalid signature. Sign the final
installation path with `codesign --force --sign - PATH`, then verify with
`codesign --verify --strict --verbose=2 PATH`. Check the installed path and
`--version` before launching work. Keep the previous executable recoverably
when upgrading.

## Provider or model unavailable

Run `ca --doctor` for local readiness and `ca --list-models` for configured
routes. Resolve referenced environment variables without printing their values.
Confirm the exact endpoint, protocol, and model ID. `ca --doctor --connect`
makes isolated provider requests; use it only when that provider usage is intended.
A stale live catalog is distinct from configured model availability. An
unavailable tool can indicate a mode/catalog mismatch; inspect the selected
mode and use `/errors` for bounded diagnostics.

## Saved session busy or recovery branch failure

Session writer ownership protects history across processes. Finish the active
writer or select another session; do not delete lock files to force access.
After a failed/cancelled replacement, inspect the source session and recorded
branch status. Partial output and recovery errors remain evidence of an
incomplete operation. Fix the reported storage/permission issue before retrying.
Use `--no-session` for deliberately ephemeral work, not as a repair for saved
history corruption.

## Terminal feels slow or controls behave unexpectedly

Confirm you are executing the expected binary with `command -v ca`. Existing
processes retain the previous executable after an upgrade. Record terminal
size, session length, and the exact interaction. Render tests alone do not
validate the observed input path. Long history uses bounded transcript caches;
a long-duration latency/RSS claim requires the actual soak measurement.

## Automation needs attention

Read `ca automation-engine status UUID`, then reconcile the same occurrence
when necessary. Keep its run ID and receipts. A dirty/advanced target branch,
changed review evidence, held resource, stale profile, or unknown side effect
can prevent acceptance. Fix that specific binding or obtain a fresh preview;
do not create a duplicate occurrence to conceal an uncertain mutation.
`ca automation-engine doctor --repository PATH --delegate-profile FILE` checks
missing Git metadata and stale delegate bindings without changing them.

## Tests fail under a restricted runner

Some offline tests still need loopback sockets, subprocess execution, or a
platform sandbox. Permission errors under a restricted harness are environment
failures. Rerun in an appropriate environment and preserve both logs. A passing
focused rerun is not a passing workspace. The publication report separates
fresh evidence from historical records and outstanding duration/live-model gates.

## Support evidence

`ca config explain` reports setting provenance. `ca support-bundle` creates a
private metadata-only archive. Review any archive and logs before sharing them.
Do not include runtime configuration values, credentials, private conversation
bodies, or local recovery blobs in a public issue.
