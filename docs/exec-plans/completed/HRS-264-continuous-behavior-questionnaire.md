# HRS-264 — Continuous behavior questionnaire and Markdown steering

Status: COMPLETE and installed (2026-10-07)

## Goal

The user requests a continuously runnable, self-learning AI harness questionnaire
covering all aspects of CA's durable behavior, particularly steering its personality
and working habits through Markdown. Deliver an editable human preference
questionnaire, real CA prompt integration, a 48-question evidence audit, persistent
adaptive investigation priorities, and bounded continuous operation.

## Design and acceptance

- `ca harness questionnaire init|steering|show|run|watch|status` supports profiles.
- `personality.md` contains human answers across 12 behavior dimensions. Blank
  comments are inert, initialization preserves existing answers, and named/bare
  profiles do not inherit another profile's personality.
- Main interactive and headless requests include the active human steering.
  Interactive prompt epochs freeze it across a logical turn and its retries.
- Forty-eight stable audit questions require observable evidence. Sparse telemetry
  produces investigation signals, never fabricated behavioral success scores.
- Persistent priorities and backlog deduplicate independent tasks, re-evaluate
  corrected feedback, bind workspace/build/route cohorts, and feed request focus
  and CA's AI Analyze prompt. Existing publication review remains applicable.
- `watch` performs bounded local cycles, supports Ctrl-C and finite test cycles,
  and reports meaningful changes. No new provider calls are needed for monitoring.
- Focused tests, CLI tests, full workspace tests, format, strict lint, release and
  standing recoverable signed installation must have recorded actual results.

## Findings

The `.agent/PLANS.md` referenced by repository guidance is absent; this plan uses
the existing execution-plan convention. The checkout had Git metadata and was
clean on inspection.
Older memory's missing-Git statement is stale. Existing harness automation tests
six coding families; broad personality questions must not pretend those tests
establish all-domain efficacy. Current harness promotion is review-bound.

## Progress

- Inspected current request epochs, headless prompt assembly, outcome evidence,
  profile paths, Analyze prompts and guarded harness publication.
- Catalog, core evidence learning and CLI delegated with disjoint file ownership.
- Markdown questionnaire, profile-specific loader, actual prompt integration,
  all 48 Analyze questions and six-question learned request focus implemented.
- Optional corrupt audit state no longer blocks human steering. File reads are
  nonblocking and occur in a bounded worker outside the TUI event loop.
- Actual TUI failure-first probe exposed explicit `/retry` rebuilding personality
  from the edited file. Added exact user content/timestamp-bound steering cache
  across retry branches, including empty steering; replaced sessions clear it.
- Unchanged actual TUI rerun passes all four cases on debug SHA-256
  `2c7fbbfd1089f64cac99e2c08a1fa09f5be0ed8164d26de1d0c17b722338f035`.
- Full matrices exposed steering-event ordering and a workflow test consumer
  regression. Publication now follows successful exact-tool preparation; the
  workflow fixture consumes the new event. Both focused regressions pass.
- Full default execution also exposed inherited nonblocking mode in the
  loopback fixture's accepted socket. Explicit blocking mode retains the
  existing deadline and request assertions; all eight CLI tests pass.

## Validation

Focused core tests: seven passed. Initial personality tests: three passed;
an added corrupt-optional-state regression is included in the full suite.
Eight CLI integration tests pass, including actual loopback headless prompt
capture, profile isolation, no provider traffic on invalid steering, watch
deduplication and Ctrl-C. Earlier compile failures remain in the logs.

The first default workspace matrix passes before the final retry repair; it is
retained separately and does not establish the updated source's default gate.
The first all-feature matrix completes with those two stream-consumer failures;
both are now repaired. The first current-source default matrix's remaining
failure is an unchanged existing stateful Git-filter fixture (counter 1 instead
of 2); its focused rerun passes. The complete sequential default/all-feature
rerun passes: 3,830/3,979 tests, zero failed, 43/44 existing ignored, 27/28 targets,
zero filtered. Formatting and strict all-target/all-feature workspace Clippy pass
in session 96930 (exit 0). All earlier logs are retained. Background
release/terminal/install job 42933 completes exit 0 after these gates.

An independent input audit adds the unchanged `rust-toolchain.toml` and
`clippy.toml` to the original 1,074-file inventory. Both hashes match the prior
BUG-262 receipt and Git HEAD. The complete 1,076-file manifest and metadata-only
completion receipt preserve all original rows and evidence.

Debug actual TUI: first pass three of four, explicit retry failed; unchanged
post-repair rerun four of four. Live reload, real harmless tool continuation,
empty Ask catalog preservation during retry, Escape/Ctrl-C, successful terminal
exit and termios restoration are covered. Rerun Escape/Ctrl-C measurements are
4.527 ms/29.072 ms, a bounded probe rather than long-session latency evidence.
First failure and passing receipts remain under `scratch/questionnaire/`.

All four final signed-release terminal cases pass against SHA-256
`077fec4e283c74cffe9971d506052da6aa8ea9f9ec2ae8e8d987d42a91088eb2`.
All terminals exit 0 and restore termios; Escape/Ctrl-C respond in
2.776 ms/3.907 ms in this bounded loopback probe.

The release is installed at `/Users/aji/.local/bin/ca` with candidate/staged/
installed hash equality, post-copy signing, owner/mode/version/help checks and
verified prior-binary recovery at
`/Users/aji/.local/bin/.ca-release-stage.gSNEjM/ca.recovery-ORC258-20261007T134212Z-f20a97ab23bdec8e01c5f656bacd872cec5488181e73afc78c4d545831971fa0`.
The 1,076-file complete input manifest remains unchanged through the release
build and installation. Records: `final-validation.json`, `matrix-audit.json`,
`inventory-completion.json`, `release-install.json`, `install.log`, and
`tui-release.json` under `scratch/questionnaire/`.

Installed `init`, `steering`, `run` and a two-cycle `watch` succeed. The private
editable `/Users/aji/.config/ca/personality.md` has no active answers yet and is
queued for the Codex editor. The initial audit scans 185 older-build outcomes,
excludes all 185, and has zero independent roots. Two watch cycles emit only
one first report; unchanged evidence stays quiet. Future current-build outcomes
provide a fresh baseline. `installed-questionnaire-readback.json` records this
actual installed-path check.

No paid-provider efficacy, all-question host verification or long-session soak
is claimed. The requested questionnaire, human steering integration, durable
adaptive focus and continuous local runner are delivered and installed.
