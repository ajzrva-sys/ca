# FLT-312 — Structural filtering enabled at normal startup

Status: COMPLETE — saved default enabled and signed ca 0.1.70 installed (2026-10-28).

## Outcome

The user can run ordinary `ca` without `--filter`. Added optional
`provider_filter.structural`, default false for existing configurations; when
saved true it selects the exact same structural mode as the explicit flag.
The saved `enabled` master switch remains authoritative, and `--filter` keeps
its existing process-only force-enable behavior. Default/omitted configuration
serialization is compatible. No shell alias, wrapper, tool protocol or session
format change is involved.

The user's default profile now saves `provider_filter.structural = true`.
Existing enabled/filename booleans were already true; every unrelated line and
value is retained byte-for-byte. Original configuration is recoverable at
`/Users/aji/.config/ca/config.toml.before-structural-20261029T030225Z-22e50f394aff91fa63ad4c2cf72618ed84728ccbcc4977aefcd48b25d4c10aa2`. It stays owner-private beside the configuration, not in the
repository. New launches apply the setting; running conversations keep their
startup snapshot. Existing saved sessions were not rewritten.

## Verification and release

- Failure-first tests reproduce the absent saved setting. Initial compilation
  then identified nine older full configuration fixtures requiring an explicit
  false value; their existing assertions were preserved. Failed receipts remain.
- Final focused report `scratch/check-change/runs/20261029T025612Z-48332/report.json`:
  89 all-feature filter/client/privacy/provenance checks pass, no ignored tests;
  formatting and strict binary Clippy pass. Independent review approves the
  source and activation helper: `scratch/FLT-312/independent-review-final.json`.
- Exact signed candidate CLI and wide TUI, then installed-path wide TUI pass the
  multi-file site workflow without `--filter`, with saved structural mode and
  legacy Latin configured. All 45 captured local-provider requests hide private
  fixture values; HTML/CSS/JS edits, unchanged assets and new-file readback pass.
  Both TUI cases prove visible pending updates with 192 history messages.
- Scoped activation was performed only after supported-binary qualification.
  A private recovery copy, compare-before-replace and guarded rollback preserve
  concurrent settings. Metadata-only `ca config explain` confirms the saved
  structural setting comes from the explicit default-profile file. All other
  config lines and saved-session metadata are unchanged.
- One bump, 0.1.69 → 0.1.70; one all-feature ca release build reused. Background
  check/build session 79270 and qualification/install/activation session 60451
  completed. Candidate/staged/installed signed SHA-256:
  `1390482d49bc2808cd4dafd1b74018d43436acf4f4c52e662e9cb9ec37eb2a4b`. Signature, mode/owner, version/help and exact
  installed normal-entry checks pass. Binary recovery:
  `/Users/aji/.local/bin/.ca-release-stage.ljwQPK/ca.recovery-ORC258-20261029T030220Z-c7d7ed5efc789312e9c7b9e776d07960e4f0435bb34799fd555a21a9ee3d7a53`.

No new full workspace matrix or real-provider run was needed for this bounded
startup choice. FLT-310's complete gate remains the unchanged-target baseline;
FLT-311 already qualifies the same filtering/restoration with a real provider.
No parser, transport, engine, approval or session behavior was broadened.

Changed files: `src/provider_filter.rs`, `src/agent/client_tests.rs`, root Cargo
version/lock, `docs/CONFIGURATION.md`, this plan and repository completion/index
records. Authorized runtime edit: `~/.config/ca/config.toml`, structural boolean
only. No oracle/reference files changed. Final evidence:
`scratch/FLT-312/final-assessment.json`.

Follow-up real-provider qualification **without --filter** now passes on the
installed 0.1.70 artifact: [FLT-313](FLT-313-real-provider-default-startup.md).
