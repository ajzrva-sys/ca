# FLT-318 — Make `/filter` the filter control center

Status: COMPLETE. Qualified release installed (2026-11-05).

## Baseline

Installed CA 0.1.79 before this change opened `/filter` on a manual path, with Sources, Preview, Legacy filter, and one Save button. A fresh disposable session had no project rules and no previously read files. Evidence: `scratch/FLT-318/baseline/filter-screen.txt` and `scratch/FLT-318/baseline/receipt.json`.

## What shipped

`/filter` opens an overview of what is active in this process, what is saved for the next launch, and which project or profile edits are still unsaved. Files, Rules, Preview, and Settings are rows. Arrow keys and Enter work in the lists, and each screen names its action.

Files search runs on a background worker. It uses the same bounded, ignore-aware walk as the composer picker and keeps paths with spaces and Unicode. File contents are read only when a file is opened. A path outside the index still uses the existing source check.

Rules show the stored project policy in order. The last matching rule wins. A startup override is shown and is not written back as the project default. **Save project rules** uses the existing revision and snapshot check and applies at the next request. **Save for next launch** patches only the changed `[provider_filter]` keys, publishes atomically, and does not install them into the current process. Restart loads them.

`/filter report`, `/filter on`, `/filter continue`, and the sticky stop are unchanged. Config and session formats are unchanged.

## Verification

Focused all-feature check `scratch/check-change/runs/20261105T185347Z-32390/report.json` passed:

- 214 tests, 0 failed (`tui::privacy`, `privacy::policy_store`, `privacy::selected_tests::startup_overlay`, `provider_filter::tests`, `config::`, `filter_discovery`)
- `cargo fmt --all -- --check`
- `cargo clippy --bin ca --all-features -- -D warnings`
- one `cargo build --release --bin ca --all-features`

Release artifact: `/Volumes/projects/ca/target/release/ca`, SHA-256 `d969837b28c2e26cb4d06b1339304f08e3b906626b1a2b4eec43c3721b8cc575`, version `ca 0.1.79`. The package version was not bumped after that build, so the qualified bytes stayed the candidate.

Disposable terminal run `scratch/FLT-318/qualify-run6/receipt.json` used that binary and a loopback provider:

- found `notes.md` from search before any file contents were shown
- drafted and saved a project hide rule
- the next captured request did not contain `PrivateCanary318`
- saved image placeholders for the next launch and read them back from the profile file before the next process started
- with 192 history messages, the pending preview showed request 1 and `FIRST-PENDING-318`, then request 2's latest strip showed `SECOND-PENDING-318` and not the first marker

Frames: `scratch/FLT-318/qualify-run6/pending-first.txt` and `pending-second.txt`.

## Installation

`tools/bug257_install.sh` installed that candidate at `/Users/aji/.local/bin/ca`. Ad-hoc `codesign --force --sign -` rewrites the signature, so the installed file hash is not the linker-signed candidate hash. Immediate readback:

- candidate SHA-256 `d969837b28c2e26cb4d06b1339304f08e3b906626b1a2b4eec43c3721b8cc575`
- installed SHA-256 `f9a24eb9f2673ae1c4db6d337dbcb6ca12de546da9765fe5c7d1116aedfeef17`
- `codesign -vvv` reports valid on disk
- `ca --version` reports `ca 0.1.79`
- mode `0755`, owner `aji`
- disposable `/filter` readback passed: `scratch/FLT-318/installed-readback/receipt.json`

Previous binary recovery: `/Users/aji/.local/bin/.ca-release-stage.t0mCgt/ca.recovery-ORC258-20261105T190608Z-21f02fa240a0a81a6fed527e07fbe4e7cb22819e34780ef1a6239f9cbd572b35`.

The full `./scripts/verify.sh` matrix was not run.
