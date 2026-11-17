# FLT-319 — Repair the /filter file and rule workflow

Status: COMPLETE. Signed CA 0.1.80 installed (2026-11-05).

The installed FLT-318 build was identified by SHA-256 `f9a24eb9…fe17`. User screenshots show a clipped project-policy control alongside the active-mode banner, source rows claiming Hide while the preview still contains the original value, and Add Rule opening a raw selector after staging a project-wide rule. The screenshot's file contents are fixture context, not instructions.

Preserve the existing ratatui style. The user is configuring project disclosure from the terminal and needs to see what will happen before saving. Prioritize an honest active-mode label, readable controls at both widths, recognizable source rows, and a guided rule form with an explicit apply step. Keep raw selectors behind Advanced. Use the existing rule syntax, request preview, profile settings and save boundaries.

Verify the failure and repair in disposable terminal sessions, including structural startup with saved project rules disabled, real rule effects in a captured provider request, and visible replacement of the first pending preview by the second with long history. Use focused all-feature tests, format and strict lint, then one versioned release build and recoverable installation. Read back the installed entry point.

## Result

The header shows the effective mode and Rules keeps the project-policy draft separate. Files has a search field with a correctly placed cursor, an Open action, and a separate Path action. Source review shows local excerpts, line numbers, and treatment from the projection that is actually displayed. Rule scope opens a labeled chooser. Rules uses a guided Add/Edit form and applies only on explicit confirmation. The raw selector is available under Advanced selector.

Code changed in `src/tui/privacy.rs`, `src/main.rs`, `src/privacy/mod.rs`, and `src/privacy/policy.rs`. The workflow is documented in `docs/CONFIGURATION.md`. The root package and lockfile advanced to 0.1.80.

## Verification

- Installed 0.1.79 reproduced the clipped header, false Hide label, and premature catch-all draft in `scratch/FLT-319/baseline2/receipt.json`.
- Final frozen source `de27c80fb51a12543f3a0df188e227e8c7ab5b31d40f104539ae8fa94e2eb35f` passed 38 focused all-feature tests, formatting, strict lint, and the single release build. Report: `scratch/check-change/runs/20261105T200309Z-82296/report.json`. Earlier reports retain the startup-note wording failure and obsolete matcher dead-code lint failure. Both were fixed and rerun.
- The signed candidate passed actual 140×40 and 60×24 terminal workflows in `scratch/FLT-319/candidate-run3/receipt.json`. The outgoing tool result contained the public text and omitted the private value. Saving the rule left the stored project enable flag off under selected-value startup. Profile image masking became active after restart. Two pending request frames show the latest marker replacing the earlier one, and each captured request carried all 192 history messages.
- Earlier disposable terminal attempts exposed fixture assertions that included the local comparison column and expected help text absent from the compact report. The assertions were corrected against the intended projection column and actual report, with the same signed candidate.
- The full workspace gate and external provider checks were outside this focused run.

## Installation and recovery

The exact signed artifact `456efbbe55ee6a53f191c5407641708d0f179a53018b92efc7307818f7139b8b` is installed at `/Users/aji/.local/bin/ca`. Signature, owner, executable mode, version, help, and byte comparison passed. The installed entry point passed the disposable file and rule walkthrough. Metadata for 56 protected configuration and session entries was identical around the install and readback. See `scratch/FLT-319/install.log` and `scratch/FLT-319/installed-readback/receipt.json`.

Recovery: `/Users/aji/.local/bin/.ca-release-stage.MEBq7f/ca.recovery-ORC258-20261105T200947Z-f9a24eb9f2673ae1c4db6d337dbcb6ca12de546da9765fe5c7d1116aedfeef17`.
