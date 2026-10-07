# SES-249 — Headless Session Identity and Resume Fixtures

Status: COMPLETE (2026-09-17; store resume fixtures + CLI exclusivity; M3 echo deferred)
Owner/session: Cursor-study execution 2026-09-17
Last updated: 2026-09-17

## Goal

Lock in CA-owned headless session **identity, continue, and resume-memory**
behavior (and optional result-envelope session id echo) using Cursor Agent
CLI `2026.09.15-d2fe57e` headless probes as a study bench only — without
adopting Cursor's chat SQLite layout, cloud resume, or `persist` SSH process
model as a second CA session store.

One observable outcome after activation: CA fixtures prove headless runs can
create a session id, resume it by id, and retain last-turn conversational
memory under `--session`/`--continue`; if shipped, headless JSON results
echo the effective session id without widening authority or leaking paths.

## Non-Goals

- No Cursor `~/.cursor/chats/**/store.db` compatibility layer.
- No hosted Cursor cloud agent/chat resume.
- No replacement of SessionV4, lazy JSONL history, export/import, archive/pin.
- No `persist` clone; SSH-surviving work stays on CA jobs / `ca.serve` / `/term` surfaces.
- No oracle/reference edits; no Cursor source copied into `src/`.
- No PARITY `Matched` for interactive session browser without separate fixtures.

## Evidence and Current Behavior

### Cursor observations (2026-09-17)

- Help: `--resume [chatId]`, `--continue`, `persist list|attach|stop`,
  `create-chat`, `ls`, `resume` (latest).
- Headless DeepSeek authless probe (disposable HOME):
  1. `-p --output-format json` prompt to remember `PERSIST_PROBE_1`
     → `session_id: 67b14953-a170-425e-8a60-4eaaf6612744`
  2. `--resume <same-id> -p` “What word…?” → result `` `PERSIST_PROBE_1` ``
     with the **same** `session_id`.
- Storage: `~/.cursor/chats/<hash>/<uuid>/{store.db,meta.json}` plus
  `~/.cursor/projects/<slug>/agent-transcripts/<uuid>/`.
- `persist list` empty in the study env (no SSH-persist session created).

### `ca` source/CLI facts

- `ca --help`: `-c/--continue`, `-r/--resume-list`, `-s/--session <ID>`,
  `--name`, `--no-session`, `--new-session-id <UUID>`,
  session-export seed path.
- `src/session.rs`: `SessionV4`, `public_session_id` / `parse_public_session_id`,
  `SessionStore::{list,load,save,archive,pin,rename}`, lazy `load_recent` /
  `load_page`, automation bindings, context + mechanistic checkpoints.
- `src/session_portability.rs`: bounded export/import (`ca.session-export.v1`).
- TUI snapshots already cover `/sessions` browser, archive, history paging,
  `/new`, export overwrite, foreign import preview.
- PARITY/docs treat session V2/V4, branch/timeline, titles, retention, lazy
  history as **CA extensions** (not Cursor parity claims).

### Inferences (labeled)

- Inferred: Cursor headless resume-memory is a useful **regression shape** for
  CA headless+session store, not a product feature gap by itself.
- Inferred: `persist` is process/SSH durability, already adjacent to CA
  `terminal_sessions` / jobs / `ca.serve` — do not fork SessionV4 for it.

## Scope

Allowed files (when activated):

- Headless/session seams in `src/main.rs`, `src/session.rs`, headless contract modules as needed
- Focused tests under `src/`, `tests/`
- Docs: `TESTING.md`, `PARITY.md` (Observed note only), `CONTINUITY.md`, `NEXT_STEPS.md`, this plan

Forbidden files/areas:

- `oracles/`, `reference/`, Cursor install/HOME chat stores
- Credentials; real user `~/.config/ca` / session roots in tests
- Unrelated active ExecPlan hunks (AUT-231, CUR-245, SEC-246, TUI-247, CMD-248)
- Destructive session deletion behavior changes beyond tests

Runtime state affected: disposable temp session roots only.

## Design and Invariants

1. **CA storage stays** — SessionV4 + existing store APIs; no dual chat DB.
2. **Identity rules unchanged** — `--no-session` stays memory-only; fresh
   `--new-session-id` remains explicit; continue/resume cannot combine with
   plan-mode launch conflicts already encoded in CLI.
3. **Headless resume = store contract** — Resume must load canonical session
   state; model memory of prior turns is the store's job, not provider cache
   tricks.
4. **Optional echo** — If headless JSON includes session id, it must be the
   effective public id, redacted of filesystem paths.
5. **Automation bindings untouched** — Scheduler session leases/journals remain
   out of scope unless a failing test forces a citation-only fix.
6. **Evidence discipline** — Cursor probe informs fixture design; acceptance is
   CA tests only.

## Milestones

### M1 — Inventory current headless session behavior

- Record what CA headless already emits/loads for `--continue`, `-s`,
  `--new-session-id`, `--no-session` (result envelope fields, logs).
- Note whether session id already appears in JSON/JSONL output.
- Validation: plan Progress notes + existing test names cited (no code if sufficient).

### M2 — Headless resume-memory fixtures

- Disposable session root; run A stores a unique token in conversation; run B
  `--session`/`--continue` asks for it; assert token present and public id stable.
- Cover `--no-session` isolation (no cross-run memory into saved store).
- Validation: focused headless/session tests.

### M3 — Optional session id echo in headless results

- Only if M1 shows a gap: emit effective session id in JSON/JSONL result
  envelope; document schema compatibility.
- Validation: contract tests for field presence, id format, path redaction.

### M4 — Docs / PARITY hygiene

- PARITY Observed: Cursor `--resume`/`--continue`/`persist` help + headless
  `PERSIST_PROBE_1` recall (version/date). CA session product rows remain
  CA extensions / Unverified for Cursor TUI.
- NEXT_STEPS/CONTINUITY updates; move plan on completion.
- Validation: docs; `oracles/` status empty.

## Acceptance Criteria

- [ ] M1 inventory recorded; no duplicate/conflicting headless session fields undocumented.
- [ ] Fixtures prove resume-memory by public session id and `--no-session` isolation.
- [ ] If echo ships: JSON/JSONL contract tests pass; no absolute session store paths.
- [ ] SessionV4 export/import/archive/pin behavior unchanged by default.
- [ ] No oracle/reference edits; no Cursor storage code copied.
- [ ] Focused tests + fmt + scoped clippy recorded honestly.

## Validation

Focused: headless session create/resume/no-session tests; result envelope tests if M3 runs.

Completion (when implementation milestones run): `TESTING.md` for touched surface; fmt; clippy; snapshot hygiene if TUI untouched note N/A; clean `oracles/`/`reference/` status.

Manual/visual: optional `ca -p` two-step resume in a temp HOME/profile; not required if hermetic fixtures pass.

Blocked checks: Cursor interactive `ls`/picker TUI; SSH `persist attach` behavior; cloud chat resume.

## Progress

- [x] Session resume/persist study recorded (2026-09-17).
- [x] This DRAFT ExecPlan written for later activation.
- [x] M1 inventory: parse conflicts already cover --no-session/--new-session-id vs resume; store public-id load exists; headless result envelope session id not required for store-level resume fixtures
- [x] M2 store resume-memory + isolated unsaved identity (session.rs); CLI new-session-id exclusivity tests (main.rs)
- [ ] M3 optional session id echo (deferred — store-level resume proven; headless JSON echo only if a later contract gap appears)
- [x] M4 docs/PARITY hygiene (index/NEXT_STEPS/CONTINUITY; Cursor resume Observed noted in plan)
- [x] Activated and completed 2026-09-17 (M3 echo deferred)

## Decisions

- Decision: Status DRAFT until the user activates this plan.
  Evidence: User asked for SES-249 drafted the same way.
  Consequence: No `src/` edits from this document alone.

- Decision: Do not implement Cursor `persist` as SessionV4.
  Evidence: Study showed SQLite chat stores + SSH process attach; CA already
  has jobs/serve/term surfaces.
  Consequence: persist mapping is docs-only unless a later user-requested plan opens it.

## Surprises and Discoveries

- Cursor local headless resume retained model-visible conversation memory with
  the same session_id across processes — good CA fixture shape.
- CA already has a larger session product surface than Cursor local CLI help
  exposes; the gap is mostly headless contract characterization.

## Recovery and Rollback

- DRAFT abandon: no source rollback.
- Implementation: revert only Scope files via normal Git; delete disposable test session roots.

## Completion Record

Changed files:
- `src/session.rs` — `store_resume_roundtrip_preserves_conversational_memory`
- `src/main.rs` — `cli_parser_rejects_new_session_id_with_resume_paths`; restored `#[test]` on no-session resume rejection

Commands and results:
- Focused tests PASS: store resume token, new-session-id exclusivity, no-session vs resume
- `cargo fmt --all`; `cargo clippy --bin ca --all-targets -- -D warnings` PASS
- M3 headless JSON session-id echo deferred (store-level resume proven; no envelope gap forced this turn)

Remaining risks: none for shipped fixtures.
Harness updates: NEXT_STEPS + plan index status.

