# Responses reasoning summary privacy

Status: COMPLETE
Owner/session: grok plan-stage privacy block
Last updated: 2026-11-16

## Goal

A plan author on `xai-oauth` / `grok-4.7` can continue when a prior Responses reasoning item has readable `summary` text that names a private filename. The status line must not become:

`Plan stage ended Failed: privacy request blocked: Private content remained in an immutable protocol field at input.*.summary.*.text; use /filter continue for a fresh privacy-safe continuation`

Encrypted reasoning ciphertext stays exact. A different private string that is not projected still fails the wire check.

## Non-Goals

- Do not start held exec plans. AGT-271 M5 stays finished.
- Do not weaken `validate_wire` so private summary text can leave the host.
- Do not rewrite `encrypted_content`, item ids, or tool names.
- Do not mutate `~/.config/ca` or running sessions while diagnosing.
- Do not copy oracle source. Do not edit `oracles/`, `reference/`, or `ca-hrs-252/`.

## Evidence and Current Behavior

Screenshot 2026-11-16 15:10: CA plan session `Plan stage canonical text mismatch fix - grok`, model `grok-4.7 (high)`, `Project privacy`, `filenames on`. The composer still held `testing`. The notice is the immutable-field sentence at `input.*.summary.*.text`.

Source facts:

- `Context::prepare_request` in `src/privacy/mod.rs` masks message text and function-call arguments. For `ResponsesOutput` items it only copies projected `function_call` arguments. Reasoning `summary` / `content` text is left as the provider stored it.
- `openai_responses_request` in `src/agent/provider.rs` replays those items as `OpenAiResponsesInputItem::Output(item.clone())`.
- `validate_wire` walks non-control strings. `wire_control_field` treats `input.*.summary.*.type` as control and `text` as data. Remaining private text returns `immutable_wire_error`.
- Existing Grok fixtures store empty summaries or omit `summary`. Live grok-4.7 fills `summary_text`.

Inferred: Grok's reasoning summary is plaintext, like function-call arguments, not ciphertext. The outgoing request copy can project it. Canonical session history stays the provider item.

## Scope

Allowed files:

- `src/privacy/mod.rs`
- `src/privacy/filename_tests.rs`
- `docs/exec-plans/active/responses-reasoning-summary-privacy.md`
- `CONTINUITY.md` and `NEXT_STEPS.md` after the first red, then on completion

Forbidden files/areas:

- `oracles/`, `reference/`, `ca-hrs-252/`
- `~/.config/ca`, session files
- Held plans under `docs/exec-plans/active/` other than this file

Runtime state affected: one replace of the installed `ca` through the standing post-task rule after the focused check passes. Leave running sessions alone.

Interfaces: privacy request preparation and Responses replay items. No session-format change.

## Design and Invariants

After the canonical replay check, the request-only copy of a same-route reasoning item must run `mask_known` on `summary[].text` and `content[].text`. `encrypted_content`, ids, types, and call identity stay exact. `validate_wire` keeps rejecting unprojected private strings in those fields.

## Milestones

### M1 — Red fixture

Add `responses_reasoning_summary_text_is_projected_before_wire_validation` in `src/privacy/filename_tests.rs`. Prepare a grok-shaped continuation whose reasoning summary names the fixture private path. `validate_wire` on the Responses `input` must succeed, the sent summary must be an alias, and ciphertext must be unchanged.

Expected red: `Private content remained in an immutable protocol field at input.*.summary.*.text`.

Validation: `python3 scripts/check-change.py --selector responses_reasoning_summary_text_is_projected_before_wire_validation`

### M2 — Project reasoning plaintext

Smallest change in `prepare_request` so same-route reasoning items are projected before the body is built. Re-run M1.

### M3 — Release

One aggregate check-change for the new selector, then the standing signed install of that artifact.

## Acceptance Criteria

- `responses_reasoning_summary_text_is_projected_before_wire_validation` passes.
- Sent `input.*.summary.*.text` does not contain the private filename and does contain a `__CA_PATH_` alias.
- `encrypted_content` equals the stored ciphertext.
- `validate_wire` still fails when that summary is left unprojected.

## Validation

Focused: the selector above.
Completion: that same focused check-change, then signed install.
Manual/visual: a grok-4.7 plan continuation after reasoning is not required for M2; the fixture is the acceptance case.
Blocked checks: full `./scripts/verify.sh` is not this task.

## Progress

- [x] M1 red fixture
- [x] M2 projection
- [x] M3 install

## Decisions

- Decision: project summary/content text on the outgoing copy; do not skip those fields in `validate_wire`.
  Evidence: skipping would send private names. Function-call arguments already use this request-only projection.
  Consequence: Grok still receives encrypted state; readable summary uses aliases.

## Surprises and Discoveries

- Live grok-4.7 fills reasoning summaries. Prior fixtures used `summary: []`.

## Recovery and Rollback

Idempotence: the focused selector is safe to repeat. Do not rebuild a different candidate after the install artifact is chosen.
Recovery: restore the SHA-labelled recovery copy created by `tools/bug257_install.sh`. Source stays in this tree; no git history rewrite.

## Completion Record

Changed files: `src/privacy/mod.rs`, `src/privacy/filename_tests.rs`, `Cargo.toml`, `Cargo.lock`, this plan, `CONTINUITY.md`, `NEXT_STEPS.md`
Commands and results:
- Red: `python3 scripts/check-change.py --selector responses_reasoning_summary_text_is_projected_before_wire_validation` failed in `scratch/check-change/runs/20261116T201749Z-74907/report.json` with `Private content remained in an immutable protocol field at input.*.summary.*.text`
- Green: same selector passed in `scratch/check-change/runs/20261116T201946Z-74949/report.json`
- Release: `python3 scripts/check-change.py --selector responses_reasoning_summary_text_is_projected_before_wire_validation --release` passed in `scratch/check-change/runs/20261116T202104Z-75014/report.json`. Candidate SHA-256 `74dace9a7a45d9d588994505ee1c2d5aa93d0f50babc7cd2438672ae2fc3b7ea`
- Install: `bash tools/bug257_install.sh`. `ca --version` is `ca 0.1.149`. On-disk readback `45250029bb12912c7e51a8a7d009450183dfd477625863c8cc6efd444107c63e`. `codesign -vvv` valid.
Artifacts (receipt paths, logs): the three check-change receipts above; recovery `/Users/aji/.local/bin/.ca-release-stage.oEBMp2/ca.recovery-ORC258-20261116T202637Z-c3f9fed6bc43545d474a3cb53f33ed8fbfcc2406a14e0355fa2ecc635422f26b`
Remaining risks: live grok-4.7 was not re-run in this session. If xAI rejects a projected summary against encrypted state, the next failure will be a provider error, not this privacy block. Full `./scripts/verify.sh` was not run.
Harness updates: `CONTINUITY.md` and `NEXT_STEPS.md` record 0.1.149.
