# Remove the provider filter and project privacy

Status: COMPLETE
Owner/session: 2026-11-18
Last updated: 2026-11-18

## Goal

A new `ca` process sends real paths, file text, and shell commands. Nothing is rewritten to `__CA_PATH_` or `__CA_NAME_`. A shell command is not rejected because it names a project path. A phrase, URL, or trip file does not stop the session. The top row no longer says Filter, Project privacy, Selected values, or filenames. The removed implementation remains on disk under `docs/archive/filter-removed-2026-11-18/` and is not compiled.

## Non-Goals

- Approval prompts stay.
- The ordinary sandbox (workspace roots, network, credential files) stays. The privacy rule that makes project sources read-only to the shell does not.
- API-key redaction in traces, and privilege-stream secret redaction, stay.
- `~/.config/ca/config.toml` is not edited. Existing `[provider_filter]` keys still load and then do nothing.
- No oracle or `ca-hrs-252/` edits.

## Evidence and Current Behavior

Screenshots from 2026-11-18 showed `Filter off · Project privacy · Selected values only · filenames on` while shell text was rewritten and then rejected. Filename mode turned on from a saved registry even when the process filter was off (`src/privacy/mod.rs`). The kill switch lived in `provider_filter::claim_session_stop` and mapped to exit 11.

`docs/archive/` is already in `.ignore`. This checkout has no root git history, so the copy is the study record.

The privacy context type also owns session alias registries and command-copy workspaces. Those copies exist to hide the real tree. The live context stays so session open and tool dispatch still compile, and every projection, stop, and shell gate is a pass-through. The archived sources are the pre-change implementation.

## Scope

Allowed files: `src/`, `docs/archive/filter-removed-2026-11-18/`, `docs/exec-plans/active/remove-filter.md`, `docs/CONFIGURATION.md`, `src/tui/FEATURE_MAP.md`, `CONTINUITY.md`, `NEXT_STEPS.md`.

Forbidden: `oracles/`, `reference/`, `ca-hrs-252/`, `~/.config/ca/config.toml`.

Runtime state affected: none until the signed binary is installed. Running sessions are not replaced.

## Design and Invariants

- `prepare_request` returns the original body. Outbound scans do not trip. `claim_session_stop` is gone. `ProviderError` has no filter-stop variant.
- `Context::enabled` and `Context::filenames_enabled` are false. Saved filename registries are not applied. `mask_known` and filename projection return the original text or path.
- Shell commands are not copied into a privacy workspace and project sources are not forced read-only.
- `[provider_filter]` still deserializes. The loaded values are not installed as an active filter.
- `/filter` does not hold a send for review.
- The transcript has no filter status row.

## Milestones

### M1 — archive
- Copy the sources and write the archive README.
- Validation: the archive directory lists the copied files.

### M2 — pass-through
- Gates above, plus tests that a workspace path, a real filename, a former stop phrase, and an enabled config do not rewrite or stop, and that the status row is absent.
- Validation: the focused tests named in Acceptance Criteria.

## Acceptance Criteria

- A shell command that names a workspace path is not rejected for privacy.
- A directory listing sent onward contains the real filename.
- A body that contains a former stop phrase is unchanged and does not stop the process.
- A config with `enabled = true` and `hide.filenames = true` still loads and does not rewrite.
- The transcript frame has no filter status row.
- `rg` over `src/` finds no `__CA_PATH_`, `__CA_NAME_`, `FilterStopped`, `claim_session_stop`, or `ProtectedSourceUnavailable`.

## Validation

Focused: `python3 scripts/check-change.py --selector` for the new tests, then one `--release` run.

Completion: `bash tools/bug257_install.sh` on that artifact.

Manual: a new process is required. This plan does not drive a live model session.

Blocked checks: full `./scripts/verify.sh` is not required. Session files still parse; new turns do not write alias records.

## Progress

- [x] Archive copy
- [x] Pass-through gates and tests
- [x] Focused check and signed install. `scratch/check-change/runs/20261118T122653Z-6694/report.json` passed for ca 0.1.181. Installed SHA-256 `81f432524aeae9bbd8b340aaae60c4ead5409142a4ca5137b5c2d0b01184349d`. Recovery of 0.1.180: `/Users/aji/.local/bin/.ca-release-stage.y1stPM/ca.recovery-ORC258-20261118T123257Z-3b7ccd171c2ea47b25dd83ee7149ea9740e3ed7b7fb388b19dfb420ed0048fb8`.

## Decisions

- Decision: keep `privacy::Context` as a pass-through type instead of deleting the module.
  Evidence: session open, tool dispatch, and jobs take `Arc<Context>`.
  Consequence: the pre-change behavior is only in the archive. Live methods that projected or stopped return the original value.

## Surprises and Discoveries

- `Filter off` still ran filename mode when a saved `filename_registry` existed.
- Comment ceilings fell in the files whose filter comments were removed. `python3 scripts/check-pyramid.py --update` lowered those ceilings.
- The first release check passed on 0.1.180. The package version was then set to 0.1.181 and the same check passed again. The installed binary is that second artifact.

## Recovery and Rollback

The archive copy is the source record. The installer keeps the previous binary as its usual recovery copy. Repeating the copy is safe. Do not edit `~/.config/ca/config.toml`.
