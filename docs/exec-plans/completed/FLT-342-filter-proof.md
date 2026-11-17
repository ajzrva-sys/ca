# FLT-342 — Filter proof

Status: INSTALLED as ca 0.1.100. SHA-256
`4fec43c00d271ed736f418ae231a4373d167b681cf6291467368f581a1b46307`.
Recovery of 0.1.99:
`/Users/aji/.local/bin/.ca-release-stage.UCPDXp/ca.recovery-ORC258-20261110T223626Z-bae74cc8d904bdf07784a06ccd820bb112639dedad39165408f635e9419bd128`.

The provider copy of an empty-file listing is already aliased. The status line,
the saved task result, and a narrow terminal hide that fact. Phases 1–4 and 6
do not change provider bytes except the host sentence in phase 6, which is a
fixed instruction. Phase 5 projects `list_dir` and `glob` success text before
the provider message and the outcome excerpt.

## Contract

- Status detail with filename mode: `Project privacy · Selected values only · filenames on · N selected values`.
- `AgentTaskAttempt.provider_summary` keeps the unrestored assistant text. Missing on old plans.
- A hidden provider pane shows one `Provider listing:` alias line. A visible pane does not.
- Pre-Send preview gains local `filename_samples`. They are not part of `provider_copy`.
- Successful `list_dir` and `glob` text that enters the next provider message and `tool_evidence` is projected. Local `model_text()` stays raw.
- Filename mode adds one fixed child instruction. `restore_with_spans` returns one span per filename alias.
- No user config, session, or `/Volumes/projects/filtertest` edits.

## Validation

Focused check passed: `scratch/check-change/runs/20261110T222806Z-12882/report.json`
(selectors for the empty-file listing, preview samples, narrow task-map footer,
and task-filter preview). Format and strict lint passed. Full `verify.sh` was
not run: phase 5 reuses `prepare_request` projection and is covered by
`project_tool_output` inside the listing test. Release install follows.
