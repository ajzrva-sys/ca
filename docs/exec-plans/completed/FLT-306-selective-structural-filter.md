# FLT-306 — selected-value structural filtering

Status: COMPLETE — signed ca 0.1.64 installed and qualified (2026-10-27).

## User request and acceptance

Make normal `ca --filter` use structural projection, preserve ordinary code and tool diagnostics, and hide only selected private values. Keep explicit project Hide/Alias/Allow rules, configured Hide/Stop and URL/image choices, stable aliases and source-bound native edit restoration. A legacy page alphabet must not re-encode structured content in this mode. No implicit runtime policy/config rewrite. Existing stored policies remain readable.

Normal entry point: installed CLI/TUI with `--filter`. Failed baseline: installed 0.1.63 `read_file` final HTTP retains Python structure with project privacy plus pages off, but re-encodes all words with pages Latin. No project policy means `--filter` previously enabled only the legacy gate. Evidence: `scratch/FLT-306-diagnosis/structural-wire-receipt.json`.

## Bounded implementation

Add a backwards-compatible policy fallback, keeping old saved policy semantics outside the explicit selected-value launch. `--filter` enables a process-only structural context with public fallback; explicit private selections remain authoritative. Disable legacy page substitution for this launch while preserving selective Hide/Stop/image/URL rules. Source projection and unbound stdout preserve unselected text; known selected identities stay masked. Keep current durability and edit bindings. Update only affected UI wording/docs.

## Verification and release

1. Failure-first focused regressions: selected identifiers/literals remain hidden in native source, stdout/history and final adapter requests; keywords/public values stay readable; exact edit restoration and save/reload retain selections.
2. Focused tests, formatting and strict lint, then required full workspace gate for shared request/policy changes. Freeze source and independently review combined result before final qualification. Preserve failures; at most three repairs for each distinct failure.
3. Actual candidate CLI/TUI loopback with conflicting legacy Latin, private selections, readable diagnostics, local edit roundtrip; pending preview second update with representative history if preview behavior changes.
4. Real native provider on synthetic code: find a planted bug using preserved code/diagnostics, make an alias-preserving edit, run real local tests, inspect final outbound trace for raw private canaries. Only ca runtime resolves normal authentication; no agent/driver reads credential values. Disposable config/sessions/workspace.
5. Advance root patch once; build intended all-feature release once, reuse exact qualified artifact for signed recoverable atomic installation and normal PATH readback. Report actual outcome and protected state.

## Progress

- [x] Installed baseline reproduced and source boundaries traced.
- [x] Narrow before-images preserved under `scratch/FLT-306/before/`.
- [x] Implement selective projection; final focused run passes 214 tests, formatting and strict lint.
- [x] Frozen independent source review approved at `11e4809deef546c24d0d001b991c7a7bcb79921de5e177f8bd3bc51c104edef5`.
- [x] Required full gate/evaluator pass after a single 0.1.63 → 0.1.64 patch bump.
- [x] Real-provider usefulness qualification before release and through the actual installed artifact.
- [x] One intended release build, recoverable signed installation, readback and completion docs.

This workspace lacks Git metadata and `.agent/PLANS.md`; follow the existing repository ExecPlan convention.

## Qualification evidence

Final focused source: `scratch/check-change/runs/20261027T180808Z-33296/report.json` (214 pass, two ignored probes). Read-only review: `scratch/FLT-306/independent-review.json`; reviewed decoder/path/edit-view findings are covered by held-out regressions. Root release digest `0b24b6792f9a7c6924ff5275c5311e66e568cc54f15317b0497c725ac3ea52d1` differs from reviewed source only in root package versions. Real route availability passed with native ca authentication, disposable HOME and a synthetic prompt; real-provider usefulness is now qualified by the final evidence below. Failed compiler/test/cache attempts remain preserved in `scratch/FLT-306/focused-*.log` and their linked focused-helper reports. The extra session reference boolean preserves no-policy process-only launches through resume/fork without writing user project policy; old references default to their prior missing-policy behavior.

## Final outcome and evidence

The complete requested change is installed. Final source digest `0b24b6792f9a7c6924ff5275c5311e66e568cc54f15317b0497c725ac3ea52d1`; signed candidate/staged/installed SHA-256 `df50db64a728723c3719b562629f62bd10743dc9ac42db2855a2c3dde29c1f0d`. Full gate and evaluator pass without exclusions or criterion changes. Both full matrices pass (4,591 default; 4,747 all-feature; opt-in ignored probes retained). Exact-artifact CLI/TUI pending freshness, native edits, resume and installed real-provider checks pass. The installed real model sees code/diagnostics with only selected private identities hidden, fixes the planted boundary bug and passes all three local tests. Production configuration/session metadata (102 files) is unchanged; prior binary is preserved at the recovery path in `install-receipt.json`.

Earlier source repair failures, two obsolete terminal-driver label checks and failed real-provider connection attempts remain preserved. The user explicitly requested another real-provider attempt; `real-provider-4/receipt.json` then passes. `real-provider-installed/receipt.json` qualifies the final installed release separately. No source or criterion was changed to turn an external connection failure into a pass. Real-provider scope is the named native route and synthetic Python task, not every supported language/provider or long-duration fuzzing. Literal spelling masking is deterministic, not arbitrary runtime data-flow tracking.

Changed files:

- `Cargo.lock`
- `Cargo.toml`
- `docs/CONFIGURATION.md`
- `src/image_content.rs`
- `src/main.rs`
- `src/privacy/literals.rs`
- `src/privacy/mod.rs`
- `src/privacy/origin_tests.rs`
- `src/privacy/policy.rs`
- `src/privacy/projection.rs`
- `src/privacy/selected_tests.rs`
- `src/privacy/store.rs`
- `src/provider_filter.rs`
- `src/session.rs`
- `src/tui/privacy.rs`

Documentation also updates CONTINUITY.md, NEXT_STEPS.md, TESTING.md, docs/CONFIGURATION.md and docs/.INDEX.md. Final evidence index: `scratch/FLT-306/final-assessment.json`; release job/log: `full-job.log`, `full-launch-receipt.json`; installation: `install-job.log`, `install.log`, `install-receipt.json`. Background full/release session 10798 and installation session 78231 completed successfully.
