# TUI-272 — Visible CA version

Status: COMPLETE — validated and recoverably installed
Date: 2026-10-10

## Goal and implementation

The user asks for CA's version number to be displayed every time. The package
already declares 0.1.0 and `ca --version` reports it; interactive screens omitted it.
A shared compile-time `ca v<CARGO_PKG_VERSION>` identity now appears in normal
and compact conversation headers, dashboard and native/standalone personality
setup. Identity takes precedence over workspace/context metadata in narrow
terminals and with long Unicode paths. Compact mode retains its compact layout;
protocol stdout remains unchanged.

Changed source: `src/tui/mod.rs`, `src/tui/render.rs`, `src/tui/dashboard.rs`,
`src/tui/personality.rs`, `src/tui/render_tests.rs`, 297 existing render snapshots,
and `tools/version_display_pty.py`. Task documentation records the accepted result.

Preserve the existing dirty checkout, runtime configuration, sessions, running
processes and adjacent executables/services. Exact production/snapshot before-images
are retained in `scratch/version-display/before/`.

## Acceptance and evidence

- [x] Failure-first tests and independent implementation/header geometry review.
- [x] All 238 render cases pass, one existing ignore; new tests cover 12/24/60/100
      columns, Unicode paths, large context, idle/active, compact and setup screens.
- [x] Independent comparison confirms 297 changed snapshots / 580 single-row
      header replacements, preserving every unrelated byte and dirty edit.
- [x] Same 17-frame real-terminal probe fails the prior installed release and
      passes signed candidate and actual installed release with clean restoration.
- [x] Corrected complete gate and independent evaluator pass on frozen source.
- [x] Standing background release, recoverable CA-only install and actual readback.
- [x] Continuity, queue and validation documentation updated; plan completed.

The corrected report at `scratch/verify/TUI-272-corrected/` passes
3979 default cases / 29 targets and
4135 all-feature cases / 30 targets, zero
failures or outer filtered cases, 117 workflow contracts,
strict checks and 9 signed runtime cases. Independent evaluation
has zero issues. Source digest is
`112470333fe2221c4abe6dedbd4452247c8f8f5fd2e8d38008facc888c73a34b`; criteria remain
`01030516d7e82208d2c70c1f6aa37f6f1c68be6f97ad30c526a82aed36e4de3c`.

The original complete gate at `scratch/verify/TUI-272-final/` remains failed.
Its unchanged migration test observed `WouldBlock` rather than `InvalidInput`;
the proposed scan-deadline explanation is unconfirmed. The corrected report
records the passing exact case and fresh complete unfiltered default matrix,
hash-verified reuse of six passing stages/manifests for the same source/artifact,
and fresh runtime acceptance. Original reports/logs remain unchanged. No existing
assertion, deadline, ignore or verifier criterion was weakened.

Managed background session 69780 exits 0. Signed rebuilt release, gate candidate and
actual `/Users/aji/.local/bin/ca` match SHA-256 `323b2d758108cf772120f0048f30f22c4d382a92f9b183e1ad4306656961177b`.
Signature, owner/mode, version/help, original recovery bytes and all 17 installed
terminal frames pass. Backup: `/Users/aji/.local/bin/.ca-automation-stage-3eik9_to/ca.recovery-automation-20261010T163902.560306Z-c95b495804d2986cfce965d61e0fe8c2a079d1d8027d81c8c03437b527695400`, SHA-256 `c95b495804d2986cfce965d61e0fe8c2a079d1d8027d81c8c03437b527695400`.
Authoritative receipt: `/Users/aji/.local/bin/.ca-automation-stage-3eik9_to/paired-install.json`.
Recovery command: `/Applications/Xcode.app/Contents/Developer/usr/bin/python3 /Volumes/projects/ca/ca/scratch/version-display/install_ca.py --recover /Users/aji/.local/bin/.ca-automation-stage-3eik9_to/paired-install.json`.

Terminal receipts are under `scratch/version-display/`; installation logs/status
are in `release-install-job/`. The PTY probe covers settled primary screens;
render tests cover active conversation state. No distinct live-stream frame,
every-overlay interaction, Cursor parity or paid-provider behavior is claimed.
Start a new CA process to load the update.
