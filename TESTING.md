# Testing

Applicable checks for the current tree. The previous validation log is archived
at [docs/archive/TESTING-before-iso337-summary.md](docs/archive/TESTING-before-iso337-summary.md).

## Commands

`mechanical_task_packet_admits_without_a_provider_request` passed in `scratch/check-change/runs/20261113T211937Z-53015/report.json`. Run it as `cargo test --bin ca mechanistic_delegation::tests::mechanical_task_packet_admits_without_a_provider_request -- --exact`.

`scratch/check-change/runs/20261113T205846Z-8083/report.json` is history. That run failed because `admit_task_packet` was missing.

Current install identity is in `CONTINUITY.md`. The qualification sections below are historical evidence. The "Historical install record" is ca 0.1.98, not the binary named in continuity.

| Command | Status | What it validates |
|---|---|---|
| `python3 scripts/check-change.py --selector <name>` | Script in `scripts/check-change.py`. Repeat `--selector`. The admission selector passed in `scratch/check-change/runs/20261113T211937Z-53015/report.json`. | `python3 scripts/check-pyramid.py` on the stable checkout, then `cargo test --locked --offline --bin ca` with those filters, then `cargo fmt --all -- --check`, then `cargo clippy --locked --offline --bin ca -- -D warnings`. A selector does not skip the pyramid stage. Receipt under `scratch/check-change/runs/`. |
| `python3 scripts/check-change.py --selector <name> --all-features --release` | Same script. `--all-features` and `--release` are argparse flags. | The pyramid stage, then the same test and clippy stages with `--all-features`, then `cargo build --release --bin ca --all-features`. |
| `python3 scripts/check-change.py --prepare-only` | Same script. | Stable checkout and cache preparation. Not a test pass. |
| `cargo fmt --all -- --check` | Listed in `docs/DEVELOPMENT.md` and `docs/QUALITY.md`. | Formatting. |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Listed in `docs/DEVELOPMENT.md`. | Strict workspace lint. |
| `cargo test --workspace --locked` | Listed in `docs/DEVELOPMENT.md`. | Default workspace tests. |
| `./scripts/verify.sh` | `scratch/verify/20261114T231943Z-8634/` exited 0. Evaluation passed. `test-default` 5191 passed, `test-all-features` 5347 passed, runtime passed. The earlier run `scratch/verify/20261114T041527Z-7761/` exited 1. | Full unattended gate. Details in `docs/VERIFICATION.md`. |
| `./scripts/verify.sh --self-test` | Documented in `AGENTS.md`. Not run in the docs pass. | Intentional-defect detection in a disposable copy. Run when that verifier changes. |
| `bash tools/bug257_install.sh` | Installer documented in `docs/VERIFICATION.md`. Docs-only work must skip it. | Signed replacement of an already-built candidate. |
| `python3 scripts/check-exec-plan.py <plan.md>` | GRD-010. Reads one plan. Does not rewrite it. | Exits 0 when Goal, Acceptance Criteria or Validation, and Recovery and Rollback are present. Otherwise prints `active ExecPlan missing required section: <name>`. |
| `python3 scripts/check-pyramid.py` | Runs before Cargo in `scripts/check-change.py` and `scripts/verify.py`. `--prepare-only` skips it. A selector does not. | Exits 0 when comment ceilings, the `src/main.rs` blocking ceiling, `src/tui/` blocking calls, and oracle `include!` / `include_str!` / `include_bytes!` / `#[path]` stay inside the baseline. `--update` rewrites ceilings only downward. |

Toolchain pin: Rust 1.97.1 in `rust-toolchain.toml`. Package version is the root `ca` entry in `Cargo.toml`.

Minimum for a small Rust behavior change: one focused `check-change.py` selector that names the changed behavior. Treat a formatter or Clippy failure as a failure. Cite `./scripts/verify.sh` only with that run's report. Docs-only harness edits do not require Cargo.

On failure: keep the failing output, change the smallest cause, rerun the same selector. `AGENTS.md` caps automatic repair at three attempts. Leave the assertion in place.

Run one Cargo stage at a time against the shared `target/`. Two stages waiting on that lock are a failed setup.

## Focused change

```sh
python3 scripts/check-change.py --selector <test-name>
```

Do not run two Cargo stages against the same target directory.

Historical gates, including the BUG-340 ca 0.1.98 install record, are in [docs/archive/TESTING-qualifications-2026-11.md](docs/archive/TESTING-qualifications-2026-11.md).
