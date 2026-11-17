# PRV-305 — Mechanical project privacy

Status: COMPLETE — source, qualification and signed installation completed
2026-10-27. Real-provider usefulness remains unverified, as recorded below.

## Outcome and accepted scope

Implemented the approved mechanical local projection. Project content is
hidden unless explicitly allowed; typed requests and project instructions stay
readable with known private values replaced. Existing projects retain legacy
behavior until project privacy is enabled. All filtering, selection,
restoration and validation are deterministic, with no additional provider call.

The local `/filter` manager provides source/structural navigation, Allow,
Alias and Hide actions, reviewed ranges, reusable selectors, explicit regions,
file patterns and Original/Sent, Changes/Full request previews. Wide layouts
place selection beside preview; narrow layouts use tabs. Saving takes effect
at the next provider boundary; cancelling preserves the current policy.

Supported formats: plain text, Markdown, JSON, YAML, TOML, CSV, HTML including
embedded JavaScript/CSS, C/C++, Rust, Python, POSIX/Bash and JS/TS/JSX/TSX.
Unknown, malformed and unparsed protected content remains opaque. Untouched
source byte ranges and formatting are preserved. Code placeholders need not be
executable; mapped edits restore originals before validation or execution.

## Completed implementation

- [x] Versioned project policies keyed by canonical workspace identity, separate
  from the session alias registry, with authoritative policy-save checks.
- [x] Opaque collision-checked aliases in private immutable segments. Session
  metadata references the committed manifest; ownership/CAS and authoritative
  readback protect publication. Children share mappings; forks inherit and
  append independently. Portable exports omit the registry.
- [x] Host-owned source receipts for native/delegated reads, attachments and
  request history. Projection precedes provider lowering; final transmission
  requires complete coverage, a current policy and validated prepared bytes.
  Chat Completions, Responses, Anthropic and Google are covered. Conflicting
  immutable replay blocks locally; safe continuation retains completed work.
- [x] Source/policy-bound mapped edit views, one-pass restoration, real-result
  validation, stale/unknown/ambiguous rejection and existing approval/drift
  checks. Protected sources are read-only to opaque shell writes; commands are
  withheld when that restriction cannot be enforced.
- [x] Revision-bound compiled matchers and budgeted reproducible caches,
  on-demand grammars, worker-based parsing/persistence and accurate final-wire
  preview. Newly private identities reconsider earlier content.
- [x] Repairable request blocking and corrected retry without repeating
  completed external actions. Legacy emergency/stop semantics remain intact.
- [x] Frozen combined review, required workspace qualification, one version
  bump, one intended release artifact and recoverable signed installation.

## Source and review evidence

Initial 699-file before-images: `scratch/PRV-305/before/` and
`baseline-manifest.json`. The necessary release-fixture probe visibility repair
in `crates/ca-automation/src/self_improvement/vault.rs` has its separately
recovered before-image and `before-image-recovery.json`. No oracle/reference
implementation was copied or edited. This workspace has no Git metadata or
`.agent/PLANS.md`; the existing ExecPlan convention was followed.

Final frozen source: `scratch/PRV-305/full-22/source-manifest.json`, digest
`9f544fa20d4255db32f85088829a728cb57170b42477b0fe2ca9cf2cc5180d5f`.
The verifier confirmed the same digest after execution. Criteria digest:
`1c366a747a73a1914ca714abe2543ff583d5502ef1b137fd656ee47889074368`.
Independent scoped reviews and their actual reviewed identities, repaired
findings and limits are recorded in `combined-review-receipt.json` and
`egress-review-receipt.json`. Authored fixes are distinguished from independent
review; these receipts do not claim every byte of the later manifest was reviewed.

## Verification

Evidence paths below are relative to `scratch/PRV-305/` unless stated otherwise.

- Focused21: `scratch/check-change/runs/20261027T155922Z-36109/report.json`:
  189 passed, two measurement probes ignored; formatting and strict ca-bin
  Clippy passed. Both original full19 regressions also passed with all features
  in `recovery-pane-all-features-21/receipt.json`; test-target strict lint passed.
- Full gate: `full-22/report.json` and `full-22/evaluation.json` PASS, no issues.
  Default workspace: 4,581 passed / 45 ignored. All-feature workspace:
  4,737 passed / 46 ignored. Formatting, strict workspace/all-target/all-feature
  Clippy, all-feature workspace release build and native runtime checks passed.
  Ignored opt-in checks remain ignored; they are listed in the evaluation.
- Behavioral coverage includes every supported format, embedded languages,
  Unicode/malformed regions, selectors/collisions, exact restoration, stale
  views, native/delegated edits, shell blocks, declassification/new identities,
  subagents/checkpoints/resume/forks and before/after-commit save failures.
  Captured final adapter requests assert synthetic private canaries are absent;
  blocked requests do not reach a socket.
- `native-candidate-22/receipt.json` and `native-installed-22/receipt.json` PASS
  all seven requested cases plus resume: headless CLI, wide/narrow TUI,
  hidden paths, repairable retry, interactive policy save and enabled privacy
  without Allow rules. Both use the same final artifact and disposable HOME.
  With 192 history messages, a second update visibly replaces the first while
  HTTP is still pending at widths 140 and 60. Evidence is decoded real PTY
  frames and terminal bytes, not a completed-result screenshot. The completed
  write's inode/mtime/ctime/size/hash survive retry unchanged; resumed aliases
  remain stable and restored local edits are exact.

## Performance

Frozen Rust fixture: `performance-source.rs`, 2,754,729 bytes / 63,351 lines,
SHA-256 `9e8c005a1a1181e6a70a4f21190fb0a546098bf3fdded7cecf90e0f1356eb148`.
`performance-release-17/budgets.json` passes unchanged budgets:

| Measurement | Result | Budget |
| --- | ---: | ---: |
| Cold worker projection | 248.311 ms | 250 ms |
| Added warm preparation p95, 1 MiB | 6.042 ms | 10 ms |
| Added warm preparation p95, 16 MiB | 45.202 ms | 50 ms |

Twenty warm continuations change the final authored marker and include adapter
lowering, final validation and complete local preview; paired filter-off time
is subtracted. Actual-client HTTP/SSE supplemental added header-arrival p95
was 1.793/35.871 ms. Background worker refresh took 312 ms with 311 event-loop
heartbeat ticks. Initial publication of 11,387 aliases used six immutable files
and 49.5 ms. Grammars load on first use, not startup.

This release-unit performance receipt precedes the later recovery/fork/pane
repairs, version bump and test-only lint corrections. Its identity is retained
separately; it is not labelled as a timing of the installed artifact. Those
later changes leave projection/matching/adapter/preview preparation algorithms
unchanged. Final-artifact normal CLI comparison is separately recorded in
`performance-cli-final.json`: five fresh processes per mode, all successful.
Median startup was 7.70 ms; filter-off 9.06 ms; strict projection 357.34 ms.
Baseline 0.1.62 startup/off/Latin medians were 8.47/9.16/87.63 ms. These CLI
measurements include process/UI output costs and are distinct from worker and
same-process warm budgets; OS caches were not flushed.

## Failures preserved and repaired

Full18 failed lint; full19 exposed legacy same-generation pane scroll and
replacement-branch privacy readiness failures. Both incomplete gate receipts
and isolated reproductions remain preserved. Pane source changes now distinguish
privacy snapshots while retaining legacy scroll behavior; replacement requests
wait for owned branch privacy readiness and preserve the frozen request exactly.
Focused21 and full22 qualify the repairs. Initial compilation/format failures,
native-debug10–12 failures and all failed performance probes (including
release07–16) remain under the task evidence directory. No failed or interrupted
attempt is represented as passing. The original chronological plan is preserved
as `plan-progress-before-completion.md`.

## Release and protected state

Version advanced once, 0.1.62 → 0.1.63. The full22 all-feature candidate was reused
without rebuilding: `full-22/candidate/ca`, installed at
`/Users/aji/.local/bin/ca`. Candidate/staged/installed SHA-256:
`1a04e685fb7b951213c0f4b24166bb2214b6c9b3f4fea3d36bf931cee119b843`.
`install-22.json` and `install-22.log` record atomic installation, ad-hoc signing
at the destination, valid-on-disk signature, owner aji, mode 0755, version/help
readback and identical hashes. The recovery copy is
`/Users/aji/.local/bin/.ca-release-stage.kL1M3F/ca.recovery-ORC258-20261027T163944Z-6a4e3ab708cb36ffe4d4ca5469fd5949fe75237e5575cfcad2809519f2bbb231`.

`runtime-after-install.json` confirms all 99 configuration/session metadata
entries unchanged, with no additions/removals. Only metadata was read for this
comparison. Tests use disposable state; real settings/credentials were not read
or copied, and running user processes were left alone. Final source changes,
identities and evidence index are in `final-assessment.json`. Documentation-only
completion updates do not require another Rust build.

## Coverage limits

Real-provider usefulness is NOT RUN: no isolated authenticated route was supplied.
Credentialless loopback proves request/edit wiring, privacy bytes, deterministic
tasks and recovery, not external-model reasoning quality or provider latency.
Host-platform qualification and the explicitly ignored opt-in checks are not
cross-platform, VM or long-duration soak proof. Token counts are estimates.
Retained structure can reveal context. Typed requests can reveal private values
before discovery; later discovery cannot undo transmission. Reasoning requiring
withheld facts remains limited, with exact-value operations performed locally.
This is a user-approved CA extension, without a Cursor parity claim.
