# ORCH-DOCS-001: Agent-Friendly Maps And Module Docs

## Purpose / Big Picture
Make the repo easier for agents to navigate by refreshing architecture/edit-routing docs, filling missing subsystem READMEs, and normalizing module-level rustdocs—without dense line-by-line commenting in `app.rs`.

## Progress
- [x] Write this ExecPlan
- [x] Add `docs/AGENT_MAP.md`; update `docs/.INDEX.md` and `ARCHITECTURE.md`
- [x] Add domain/context READMEs; refresh stale subsystem README blurbs
- [x] Normalize module rustdocs
- [x] Light `app.rs` conductor map + section banners
- [x] Validate; move to completed; CONTINUITY note

## Surprises & Discoveries
- `src/domain/role.rs` is still a docs-only stub (no types yet).
- Lock coverage is broader than the pre-integrity module comment claimed; docs now match `acquire_repo_lock` call sites.

## Decision Log
- Prefer navigation/ownership docs over per-helper commentary.
- Lock policy docs list locked CLI commands from `src/app.rs` evidence.

## Validation and Acceptance
- Passed: `cargo fmt -- --check`
- Passed: `cargo test`
- Manual path audit of AGENT_MAP links against repo layout

## Outcomes & Retrospective
Agent edit map + architecture refresh + subsystem READMEs + rustdoc Owns/Read-when style + `app.rs` section map. No runtime behavior changes intended.
