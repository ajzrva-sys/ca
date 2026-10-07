# HRS-252 — Apple Container execution backend (design note)

Status: DRAFT. Design-only. No `src/` changes. Not activated for implementation.
Date: 2026-09-19
User request: short design note — execution-backend boundary, policy mapping, which NEXT_STEPS/PARITY rows this claims.

## Claimed rows

| Surface | Row | Meaning |
|---|---|---|
| `NEXT_STEPS.md` | **HRS-252** | Executable queue entry, `Status: DRAFT` until explicitly activated |
| `PARITY.md` | **HRS-252** | CA capability extension; **no Cursor parity claim**; `Unverified` until CA-owned tests exist |

This note does not implement a backend. It freezes the contract so a future task cannot invent a fourth `SandboxMode` or copy Hermes Python.

## Question

Optional isolation for `run_command` / jobs: execute in an Apple Container Linux VM (macOS 26+, Apple Silicon, Apple `container` CLI) instead of host seatbelt, when the user wants VM blast-radius rather than host path jail only.

## Execution-backend boundary

Keep **`SandboxMode` as policy strength** (`Required` | `PolicyOnly` | `Off`). Do **not** add `AppleContainer` as a fourth mode.

Add a **separate execution backend** used only where processes are spawned:

```text
ExecutionBackend::HostSeatbelt     // current default; unchanged
ExecutionBackend::AppleContainer   // opt-in, capability-gated
```

| Owns | Does not own |
|---|---|
| `SandboxBackend` + `sandbox-exec` profiles on the host | File-tool I/O, LSP, voice, hooks, browser |
| Future container runner: policy → `container run …` argv + lifetime | Hermes terminal registry, doctor/setup/Desktop bridges |
| Jobs / `run_command` when backend = container | Docker / Modal / SSH / Daylight backends (non-goal) |
| Fail-closed when CLI or host unsupported | Silent fallback to host when container was requested |

Flow when activated:

```text
approval / tool policy
        │
        ▼
SandboxMode + SandboxPolicy   (unchanged vocabulary)
        │
        ├─ ExecutionBackend::HostSeatbelt ──► sandbox-exec ──► host process
        │
        └─ ExecutionBackend::AppleContainer ──► container run
                 (mounts / tmpfs / network from policy map below)
```

Delegation children inherit the same backend as their owning job unless a later plan says otherwise. Unattended / headless paths stay fail-closed on unsupported hosts.

## Policy mapping

| Input | Host seatbelt | Apple Container (when backend selected) |
|---|---|---|
| `roots` (RW) | profile allow write under roots | bind-mount host paths RW; writes stay in those mounts |
| `read_only_roots` | profile allow read | bind-mount RO |
| `temporary_directory` | profile allow write | `--tmpfs` **path-only** mount (scratch must be writable over RO image root) |
| `network_allowed = false` | profile deny | `--network none` |
| `network_allowed = true` | profile allow | default VM network; document that this weakens isolation |
| `hide_other_processes` | profile deny process inspect | inherent to VM; still honor the flag in host tooling that can see VMs |
| `SandboxMode::Off` | no seatbelt wrapper | container backend **refused** or only via explicit weaker confirm — never implied |
| `SandboxMode::Required` + backend missing | `SandboxError::Unavailable` | same: fail-closed, no silent host exec |
| `RunEnvironment` / secrets | apply env; redact snapshots | stage secrets as RO **directory** mounts if needed; never pass secrets on argv; clear non-allowlisted env |
| Extra container flags (future) | N/A | bind-mount-shaped flags must trip the same host-access approval path as volumes |
| Workspace persistence | host workspace dirs | task workspace/root dirs on host; **VM instances** are ephemeral |

Lifetime (portable lesson, not Hermes code):

- Container lifetime tied to owning job/process (pipe EOF or owner-checked reap).
- Drop/kill owner → stop/remove container.
- Startup sweep for orphans older than `2 ×` configured lifetime.
- Persistent host workspace survives; running VMs do not accumulate.

## Acceptance (only if/when HRS-252 is activated)

1. Default remains `HostSeatbelt`; container only when explicitly configured.
2. Unsupported host/CLI → fail-closed with actionable doctor/setup copy (SEC-246 vocabulary, not Hermes wording).
3. CA-owned hermetic tests for policy→argv mapping, mount RO/RW, network deny, lifetime cleanup.
4. Opt-in native integration only: e.g. `CA_RUN_APPLE_CONTAINER_INTEGRATION=1` on macOS 26 arm64 with `container` present.
5. No oracle source copied into `src/` (`AGENTS.md` control rule).
6. Focused tests + fmt + clippy per `TESTING.md`; no release claim in the design phase.

## Evidence and oracle boundary

| Source | Role |
|---|---|
| Hermes PR #78035 (open; user-validated production notes) | Behavioral evidence for VM isolation, mount approval, orphan lifetime — **not** merge authority for `ca` |
| `oracles/hermes-agent-2026.9.14/tools/environments/` (docker/modal/ssh/local; **no** `apple_container.py`) | How Hermes shapes backends; read-only patterns only |
| Cursor oracle | **No** VM execution-backend fixture → **no Cursor parity claim** |
| `dontdo.txt` | Containers for tool isolation are out of the “would not pursue” list; messaging bots / desktop control remain out |

This is a **CA capability extension**, oracle-informed only. Do not treat Hermes registry design, config keys, or Desktop wiring as requirements.

## Non-goals

- Multi-backend terminal registry (Docker, Modal, Singularity, SSH, Daylight).
- Web/Desktop settings bridges or `TERMINAL_*` env catalogs.
- Changing default sandbox, approval modes, or `run_command` host semantics when backend is host.
- Copying `tools/environments/apple_container.py` or any oracle implementation.
- Claiming Cursor parity or flipping PARITY rows from source inspection.

## Roadmap relation

Roadmap priority 4 still ranks native terminal safety and existing isolation work above new breadth. **HRS-252 is not scheduled work.** Activating it requires an explicit user turn that names HRS-252 and accepts a full ExecPlan under `.agent/PLANS.md` standards.

Until then: hold Hermes #78035 on its own published deadline; `ca` keeps seatbelt-only execution.
