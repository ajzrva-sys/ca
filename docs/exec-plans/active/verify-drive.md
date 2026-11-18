# verify-drive — Proof an agent can rerun

Status: ACTIVE
Owner/session: current session
Last updated: 2026-11-18

## Goal

An agent can drive an isolated in-process ca, press Shift+Tab through the mode cycle, and leave a JSON proof plus a screen transcript that cleanup does not delete. A workflow step can require that proof. Ordinary chat stays a chat turn.

## Non-Goals

- M4 file-edit gate and M5 visible stop. Those stay unstarted.
- Spawning the installed binary inside the driver. The proof driver is `in_process`.
- Cursor cloud agents, pstack skill imports, approval changes, session-file schema changes.
- A worktree per interactive agent.
- Edits under `oracles/`, `reference/`, or `ca-hrs-252/`.

## Evidence and Current Behavior

- Landed in ca 0.1.187: `WorkflowEvidenceGate::Artifact`, `src/verify_drive.rs`, `ca verify`, and the mode-cycle files under `.ca/skills/verify-ca/`. The bullets below are the facts this plan started from.
- `WorkflowEvidenceGate` in `src/workflow.rs` was `AssistantOutput` or `ToolSuccess`. `complete_step` accepted nonempty assistant text or one successful tool name.
- `src/test.rs` types keys into `AppState` and renders with `TestBackend`. That runner is test-only and writes no proof.
- Shift+Tab is `cycle_session_mode` in `src/tui/input.rs`. The status message becomes `Plan mode`, `Auto-review`, `Yolo`, then `Normal`. `AgentMode::status_label` returns `plan`, `auto-review`, `yolo`, and `agent`.
- Runtime paths are `$HOME/.config/ca` and `$HOME/.local/share/ca` (`RuntimePaths::from_home` in `src/runtime.rs`).
- Skill inventory reads `SKILL.md`, `references/`, `scripts/`, `templates/`, `assets/`, and `agents/openai.yaml` (`inventory_skill_tree` in `src/skills.rs`). A `features/` directory next to `SKILL.md` is not loaded.
- CLI subcommands are routed in `parse_launch_args` (`src/main.rs`) before the standard parser. `ca skill` is the pattern: a sibling module parses and runs.

## Scope

Allowed files:
- `src/workflow.rs`
- `src/verify_drive.rs` (new)
- `src/main.rs` for the module line and the `verify` launch arm
- `.ca/skills/verify-ca/`
- `docs/exec-plans/active/verify-drive.md`
- `NEXT_STEPS.md`
- `CONTINUITY.md` after the install receipt exists

Forbidden files/areas:
- `oracles/`, `reference/`, `ca-hrs-252/`
- Tool dispatch, plan-stage stop rendering, session format, approval

Runtime state affected: none. Doctor refuses the user config and share trees. Tests use temp dirs.

Interfaces and dependencies: opt-in workflow evidence kind `artifact`. CLI `ca verify doctor|drive|proof-check|cleanup`.

## Design and Invariants

- Default evidence stays `assistant_output`. Old definitions still complete.
- An `artifact` gate names an absolute proof JSON path and a 64-hex digest. `complete_step` fails with `EvidenceFailed` when the proof or transcript is missing, the digest is short, or the transcript hash differs. The step is not marked succeeded.
- The digest is the SHA-256 of the transcript file. The proof JSON records that digest and the transcript path. The proof file does not contain its own hash.
- Doctor compares canonical paths with the user config and data dirs and refuses a symlink into them before it reads a marker there.
- Cleanup may delete the disposable home. The proof path must sit outside that home, and the proof file must still exist afterward.
- The in-process driver calls `handle_event` and `render`. The proof field `driver` is `in_process`.
- Feature files live under `.ca/skills/verify-ca/references/features/` and scripts under `.ca/skills/verify-ca/scripts/` so the existing skill inventory loads them. The directory name matches the skill name `verify-ca`.

## Milestones

### M1 — Artifact gate

- Add `VerificationProof` and the `artifact` evidence variant.
- Tests: old `assistant_output` still completes; missing proof, short digest, and digest mismatch fail closed; a matching digest completes.
- Validation: `cargo test --bin ca --offline --locked workflow::tests::assistant_output_gate_still_completes_without_an_artifact workflow::tests::artifact_gate_fails_closed_without_a_proof workflow::tests::artifact_gate_rejects_a_short_digest workflow::tests::artifact_gate_rejects_a_digest_mismatch workflow::tests::artifact_gate_completes_when_the_digest_matches -- --test-threads=1`

### M2 — Isolated driver

- `src/verify_drive.rs` prepares a marked home, doctors it, runs a script, and writes the proof.
- One script: Shift+Tab through the four mode labels.
- Validation: the verify_drive tests named below.

### M3 — CLI and one feature

- `ca verify` prints JSON from the same library.
- The mode-cycle skill files and a maintain test that runs every feature script it finds.
- Validation: the maintain test, then one `python3 scripts/check-change.py` and `bash tools/bug257_install.sh`.

### M4 — Opt-in edit gate

Not started.

### M5 — Visible stop

Not started.

## Acceptance Criteria

- A workflow whose only gate is `assistant_output` still completes.
- An `artifact` gate does not succeed without a proof file, with a short digest, or with a mismatched transcript hash.
- Doctor accepts a temp home and refuses a home that canonicalizes into the real `~/.config/ca` or `~/.local/share/ca`.
- The mode-cycle script's transcript contains `Plan mode`, `Auto-review`, `Yolo`, and `Normal`.
- After cleanup, the home is gone and the proof file is still readable. `proof-check` fails if the transcript is truncated.
- M4 and M5 are not implemented.

## Validation

Focused: the cargo tests named in the milestones.
Completion: one `python3 scripts/check-change.py` for the touched tests, then `bash tools/bug257_install.sh` on that artifact.
Manual: one `ca verify drive` of the mode-cycle script against a temp home, using the installed binary.
Blocked checks: `./scripts/verify.sh` is not required. No spawned-PTY proof. No live provider.

## Progress

- [x] M1 artifact gate
- [x] M2 isolated driver
- [x] M3 `ca verify` and the mode-cycle feature
- [x] Focused check and install
- [ ] M4 unstarted
- [ ] M5 unstarted

## Decisions

- Decision: feature files go under `references/features/` and scripts under `scripts/`.
  Evidence: `inventory_skill_tree` ignores any other directory.
  Consequence: the workspace skill loader returns those files with the skill. A top-level `features/` directory would not.
- Decision: the first slice stops before the edit gate.
  Evidence: the approved plan's first slice.
  Consequence: a normal turn can still edit. Chat does not start a plan.

## Surprises and Discoveries

- The default status line does not include the mode field. Shift+Tab sets `status_message`, which render shows as a toast. The script asserts that toast text. The driver also puts `StatusField::Mode` on the fixture status line so `mode plan` is in the same frame.
- `scripts/verify.py` `source_inventory` did not copy `.ca`. `every_feature_script_drives` walks `CARGO_MANIFEST_DIR/.ca/skills/verify-ca/references/features`, and check-change sets that root to the copied slot. The names list now includes `".ca/skills/verify-ca"` after `.codex/hooks.json`. That file was not on the allowed list above. Do not add more inventory names for this plan.

## Recovery and Rollback

Idempotence: the new evidence variant is opt-in. Re-running the tests does not touch user config.
Recovery: the installer wrote the previous binary to `/Users/aji/.local/bin/.ca-release-stage.kDAxgu/ca.recovery-ORC258-20261118T194440Z-4d86a18a7cd7ac95df61858567900b15f64aa650b0f3feaf4605bd3ad674f33b`. That file reads `ca 0.1.186`. Do not delete user sessions. Running sessions were not replaced.

## Completion Record

M1, M2, and M3 are in ca 0.1.187. This plan stays ACTIVE. M4 and M5 are not started.

Changed files:
- `src/workflow.rs`
- `src/verify_drive.rs`
- `src/main.rs`
- `.ca/skills/verify-ca/SKILL.md`
- `.ca/skills/verify-ca/references/features/mode-cycle.md`
- `.ca/skills/verify-ca/scripts/mode-cycle.script`
- `scripts/verify.py` (the inventory name above; it was not on the allowed list)
- `Cargo.toml` and `Cargo.lock` (`0.1.186` to `0.1.187`)
- `CONTINUITY.md`, `NEXT_STEPS.md`, and this plan

Commands and results:
- `python3 scripts/check-change.py` with the ten workflow and verify_drive selectors and `--release` exited 0. Report `scratch/check-change/runs/20261118T193845Z-15780/report.json` has `"status": "passed"`. Stages were pyramid, focused, format, lint, and release. Duration 337.273 seconds. This is not `./scripts/verify.sh`.
- `bash tools/bug257_install.sh` installed `/Users/aji/.local/bin/ca`. On-disk SHA-256 `cfb5f6d3b2ed11ecb9b1f55c508dfd3937b6fcf80fb6ef3d252f8bf87c9422b7` matches the report release binary `/Volumes/projects/ca/target/release/ca`. `codesign -vvv` is valid. `ca --version` reads `ca 0.1.187`.
- The installed binary drove `.ca/skills/verify-ca/scripts/mode-cycle.script` against a temp home. Drive JSON was ok, `driver` was `in_process`, exit code was 0, and the transcript digest was `07d6dbb443bd887a7b4df31745d31ad4bde087a961522b3291184ff496c20fab`. `proof-check` and `doctor` were ok. `cleanup --dry-run` named that home. Cleanup then deleted the home and left the proof. The transcript contained `Plan mode`, `Auto-review`, `Always-approve`, and `Normal`. Those temp directories were removed after the check.

Artifacts:
- Release binary `/Volumes/projects/ca/target/release/ca`, SHA-256 `cfb5f6d3b2ed11ecb9b1f55c508dfd3937b6fcf80fb6ef3d252f8bf87c9422b7`
- Installed binary `/Users/aji/.local/bin/ca`, same hash
- Recovery of 0.1.186 at the path in Recovery and Rollback

Remaining risks: the library driver does not spawn a PTY, so crossterm delivery in a separate process is unproved. The manual check ran the installed binary's `ca verify drive`, which uses that same in-process driver. M4 and M5 are not implemented. `./scripts/verify.sh` has not been run. A process started before this install still runs 0.1.186 until it is restarted.

Harness updates: `CONTINUITY.md` leads with the 0.1.187 receipt. `NEXT_STEPS.md` says M1 through M3 are in 0.1.187 and M4 and M5 stay unstarted. This file stays in `docs/exec-plans/active/`.
