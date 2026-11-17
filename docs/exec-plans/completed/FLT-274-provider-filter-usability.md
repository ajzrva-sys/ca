# FLT-274 Provider filter usability

## Purpose

Make the filter readable. An existing config file keeps the behavior it has
today. The yellow `FILTER ON` row stays those two words. A trip stays stuck
for the process. Tools still open the real file and fetch the real URL. No
model picks replacement words. The live config stays `enabled = false`.

## Config

New files use three tables. The flat keys still load.

- `[provider_filter.stop]` holds phrases, URLs, and rules. A stop always
  stops. `mask = true` also rewrites that entry on the way out.
- `[provider_filter.hide]` holds words, pairs, and URLs. A hide never stops.
  A new hide URL is exact unless `prefix = true`.
- `[provider_filter.pages]` is `off`, `latin`, a word list, or a table.
  `images = true` is the picture placeholder.

The same text in a new stop table and a new hide table fails load. The error
quotes the text and names both sides. A word in both legacy `phrases` and
`obfuscation_patterns` becomes one masked stop. Legacy URL adjustments keep
child-path rewrites. Setting both a new table and the old key for the same
list fails load and names both keys.

## Screen and commands

The row under `FILTER ON` is the armed summary. After a trip it is
`stopped on "…"`, with origin `request`, `reply`, or `fetched page`.
`/filter` opens that list. `/filter on` arms this process and does not write
the file. There is no off switch. `ca filter check` prints the same report
and can try one sample. It does not install the filter.
`ca --filter` forces `enabled = true` for that process and does not write
the file.

Provider errors, headless exit 11, and the withheld tool string keep the
rule id. The matched text stays on the local status row and in `/filter`.

## Validation

`cargo test --bin ca -- provider_filter::tests active_filter_keeps provider_copy_pane filter_summary diagnostics_subcommands tui::commands::tests -- --test-threads=8`
passed 95 tests on 2026-10-14. Full `./scripts/verify.sh` was not required
for this pass. The user's live `~/.config/ca/config.toml` was not modified.

Follow-up the same day: `/filter on` passed the same filter plus
`filter_command_rejects`, 101 tests. The config file was not modified.
