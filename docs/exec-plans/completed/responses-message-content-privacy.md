# Responses message content privacy

Status: COMPLETE
Owner/session: grok plan-stage privacy block
Last updated: 2026-11-16

## Goal

A plan author on `xai-oauth` / `grok-4.7` can continue when a prior Responses message has readable `content` text that names a private filename. The status line must not become:

`Plan stage ended Failed: privacy request blocked: Private content remained in an immutable protocol field at input.*.content.*.text; use /filter continue for a fresh privacy-safe continuation`

Encrypted reasoning ciphertext stays exact. A private string that is not projected still fails the wire check.

## Non-Goals

- Do not start held exec plans. AGT-271 M5 stays finished.
- Do not weaken `validate_wire` so private message text can leave the host.
- Do not rewrite `encrypted_content`, item ids, or tool names.
- Do not mutate `~/.config/ca` or running sessions while diagnosing.
- Do not copy oracle source. Do not edit `oracles/`, `reference/`, or `ca-hrs-252/`.

## Evidence and Current Behavior

Screenshot 2026-11-16: CA plan session, model `grok-4.7 (high)`, `Project privacy`, `filenames on`, `0 selected values`. The composer still held `testing`. The notice is the immutable-field sentence at `input.*.content.*.text`.

Session `06245868-9fe5-4b75-93ea-b81bae695a36` failed with that sentence. Author outcome `172c0510-ef60-4848-a7f0-4cecf97112dd`: `list_dir` succeeded and the listing was withheld, `glob` succeeded, two `read_file` calls failed with `Use a complete discovered filename alias`, then the next provider request was blocked.

Source facts:

- `project_responses_reasoning_plaintext` in `src/privacy/mod.rs` runs `mask_known` on reasoning `summary` and `content` text.
- `project_responses_message_plaintext` runs `project_known_spellings`, which skips a snapshot relative shorter than 8 bytes or lacking `.` and `/`.
- `validate_wire` calls `mask_known`. That uses `project_text`, which aliases census names of any length.
- The saved filename registry for this prepare contains the tokens `output_text` and `summary_text`. It does not contain `reasoning_text`.
- That census has 86 entries, and 39 are shorter than 8 bytes.

Inferred: the blocked string is assistant `output_text`, not reasoning content. The outgoing message copy can project it. Canonical session history stays outside this request copy.

## Scope

Allowed files:

- `src/privacy/mod.rs`
- `src/privacy/filename_tests.rs`
- `docs/exec-plans/active/responses-message-content-privacy.md`
- `CONTINUITY.md` and `NEXT_STEPS.md` after the first red, then on completion
- `Cargo.toml` and `Cargo.lock` only through `scripts/bump-release-version.py`

Forbidden files/areas:

- `oracles/`, `reference/`, `ca-hrs-252/`
- `~/.config/ca`, session files
- Held plans under `docs/exec-plans/active/` other than this file

Runtime state affected: one replace of the installed `ca` through the standing post-task rule after the focused check passes. Leave running sessions alone.

Interfaces: privacy request preparation and Responses replay items. No session-format change.

## Design and Invariants

After the canonical replay check, the request-only copy of a same-route message item must run `mask_known` on `content[].text`. `encrypted_content`, ids, types, and call identity stay exact. `validate_wire` keeps rejecting unprojected private strings in those fields.

## Milestones

### M1 — Red fixture

Add `responses_message_content_text_is_projected_before_wire_validation` in `src/privacy/filename_tests.rs`. Prepare a grok-shaped continuation whose assistant `output_text` names a 4-byte fixture file `n.rs`. `validate_wire` on the Responses `input` must succeed, the sent text must be an alias, and ciphertext must be unchanged.

Expected red: `Private content remained in an immutable protocol field at input.*.content.*.text`.

Validation: `python3 scripts/check-change.py --selector responses_message_content_text_is_projected_before_wire_validation`

### M2 — Project message plaintext

Smallest change in `project_responses_message_plaintext` so same-route message items use `mask_known` before the body is built. Re-run M1.

### M3 — Release

Bump the patch once, then one release check-change for the new selector, then the standing signed install of that artifact.

## Acceptance Criteria

- `responses_message_content_text_is_projected_before_wire_validation` passes.
- Sent `input.*.content.*.text` does not contain `n.rs` and does contain a `__CA_PATH_` or `__CA_NAME_` alias.
- `encrypted_content` equals the stored ciphertext.
- `validate_wire` still fails when that message text is left unprojected.

## Validation

Focused: the selector above.
Completion: that same focused check-change with `--release`, then signed install.
Manual/visual: a grok-4.7 plan continuation after a short filename in assistant text is not required for M2; the fixture is the acceptance case.
Blocked checks: full `./scripts/verify.sh` is not this task.

## Progress

- [x] M1 red fixture
- [x] M2 projection
- [x] M3 install

## Decisions

- Decision: project message `content[].text` with `mask_known`; do not skip those fields in `validate_wire`.
  Evidence: `project_known_spellings` leaves short census names in the replay item, and `validate_wire` still sees them.
  Consequence: Grok receives aliases in readable assistant text. Ciphertext stays exact.

## Surprises and Discoveries

- The home-directory census for the failing session is partial and includes many names shorter than 8 bytes.

## Recovery and Rollback

Idempotence: the focused selector is safe to repeat. Do not rebuild a different candidate after the install artifact is chosen.
Recovery: restore the SHA-labelled recovery copy created by `tools/bug257_install.sh`. Source stays in this tree; no git history rewrite.

## Completion Record

Changed files: `src/privacy/mod.rs`, `src/privacy/filenames.rs`, `src/privacy/filename_tests.rs`, `Cargo.toml`, `Cargo.lock`, this plan, `CONTINUITY.md`, `NEXT_STEPS.md`
Commands and results:
- Red: `python3 scripts/check-change.py --selector responses_message_content_text_is_projected_before_wire_validation` failed in `scratch/check-change/runs/20261116T220820Z-78142/report.json` with `Private content remained in an immutable protocol field at input.*.content.*.text`
- Green: same selector passed in `scratch/check-change/runs/20261116T220921Z-78185/report.json`
- Release: `python3 scripts/check-change.py --selector responses_message_content_text_is_projected_before_wire_validation --release` passed in `scratch/check-change/runs/20261116T221026Z-78262/report.json`. Candidate SHA-256 `de652aa78eb0ce891d1bff06586d834068d42ab8374e6ba5e0082d7bafdaf1a5`
- Install: `bash tools/bug257_install.sh`. `ca --version` is `ca 0.1.154`. On-disk readback `4b5adf91327a987103a45acbc9d0fb328c92e686c589a64e19dc91e0aa91a9c1`. `codesign -vvv` valid.
Artifacts: the three check-change receipts above; recovery `/Users/aji/.local/bin/.ca-release-stage.mkVtRm/ca.recovery-ORC258-20261116T221605Z-9a37537bc38d945d56225812914bf185b972320722c6f4e7cc4074f8b14a663f`
Remaining risks: live grok-4.7 was not re-run in this session. A failed plan does not resume. Full `./scripts/verify.sh` was not run.
Harness updates: `CONTINUITY.md` and `NEXT_STEPS.md` record 0.1.154.
