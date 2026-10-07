# LOC-089 — Managed llama.cpp and GGUF

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12

## Goal

Add an explicit profile-scoped local-model workflow that imports or exact-downloads validated GGUF files, resolves a fully reverified EXT-088 `llama-server` entrypoint, launches one loopback-only process under bounded supervision, waits through loading to a verified health/model response, registers that provider only for the explicitly requested `ca` run, reports safe logs and memory/disk estimates, and stops the full owned process group on run exit or cancellation.

## Non-Goals

- Auto-discovering/importing arbitrary GGUF files, restoring a deleted provider, auto-starting a server, background downloads, or downloading a model without exact confirmation.
- Installing llama.cpp outside EXT-088, executing an archive/install script, using Docker, attaching to remote servers, or replacing `/provider discover` for user-owned servers.
- Persisting a `local` provider/default, daemonizing a server, keeping it alive after `ca`, or changing existing LSP/tmux/browser/runtime discovery.
- LoRA/adapters, multimodal projectors, speculative decoding, arbitrary extra server flags, GPU tuning, benchmark claims, or model recommendations.
- Claiming Cursor or Ante runtime parity.

## Evidence and Current Behavior

- EXT-088 now exposes `external_runtime::resolve_entrypoint`, which revalidates one exact managed runtime receipt, complete file set, owner-only modes, sizes, hashes, target, and containment before returning a path. Installed bytes remain inert unless this new boundary deliberately spawns them.
- `src/provider_discovery.rs` already proves no-proxy/no-redirect literal-loopback model discovery and safe model IDs, but it scans user-owned services and has no process/model ownership.
- `src/main.rs::run_headless` and `run_tui` load mutable in-memory `CaConfig` before selecting the provider/model. They can receive one process-only provider without patching TOML, defaults, or deleted built-ins.
- `src/browser.rs`, `src/lsp.rs`, and `src/jobs.rs` provide process-group cleanup patterns. The sandbox abstraction can grant network only as a whole; a signed managed server therefore needs a distinct explicit start action, exact loopback bind arguments, no ambient credential environment, and no claim that sandbox policy itself restricts egress.
- Ante documentation describes a managed pinned llama.cpp engine, explicit model selection, loading/health states, local provider registration, memory estimates, and owned-vs-external shutdown. This is documentation evidence only; LOC-089 is a `ca` product extension.

## Scope

Allowed files:

- A new `src/local_models.rs`, `src/main.rs`, `src/runtime.rs`, central CLI/config-safe types required for explicit local launch, focused CLI tests/fixtures, and standard harness documents.
- `Cargo.toml`/`Cargo.lock` only if a bounded GGUF parser dependency is demonstrably safer than a small header/metadata reader.

Forbidden files/areas:

- `oracles/`, `reference/`, real config/data/Keychain/sessions, EXT-088 installed bytes, external services, system package managers, and the installed `ca` binary.

Runtime state affected:

- Explicit import/download writes only under the selected profile's `models/` store. An explicit local run creates one temporary process/log directory and one process-owned loopback server; both are removed/stopped on completion. Provider registration is in memory only.

## Design and Invariants

- Add leading `ca local` management commands: `list`, `inspect ID`, `import PATH --id ID --confirm ID`, exact `download` with HTTPS/literal-loopback URL plus declared SHA-256/bytes/license/destination preview and confirmation, and `remove ID --confirm ID`. Bare rejects persistent model mutations; read-only list/inspect may use default profile only through an explicit non-bare invocation.
- Model records are owner-only, strict, profile-scoped, and atomic. Import reads one regular non-symlink user-selected file, requires the `GGUF` magic and supported little-endian header version, streams it through a size cap and SHA-256 into private staging, refuses overwrite, and keeps only a safe source basename—not the original path. Download uses no proxy/redirect/auth/query/fragment, streams through the declared post-decompression byte/hash bound, and writes the same record shape. Neither path interprets tensors or executes embedded content.
- Safe inspection reports exact disk bytes, header version/tensor/metadata counts, SHA-256, and clearly labelled coarse minimum/recommended RAM estimates. It does not claim precise KV-cache/GPU memory without validated architecture metadata.
- Add explicit standard-run flags `--local-model ID` and `--local-runtime ID` (plus bounded context/startup options). They conflict with scripted tests, doctor/list-models, resume/continue, explicit saved-provider selection, and background service modes. No local process starts without `--local-model`.
- Local launch resolves the model from the exact profile store and resolves entrypoint `llama-server` from the exact EXT-088 runtime. It rechecks both immediately before spawn, then uses fixed arguments only: literal `127.0.0.1`, one reserved local port, exact model path, bounded context, `--jinja`, and no user-supplied extra flags.
- The child receives a cleared environment with a minimal non-secret PATH/TMPDIR, no provider/MCP/Keychain values, piped stdout/stderr, a new process group, and the effective required sandbox over only the runtime/model/temp roots. Because the current sandbox's required network grant is broad, the explicit local launch is the authority boundary and the limitation is shown in help/docs; the process is still forced to advertise on literal loopback.
- One supervisor bounds each output stream with truthful head/tail omission, polls only `http://127.0.0.1:<port>/health` and `/v1/models` with no proxy/redirect/auth, treats 503 as loading, and requires a ready response plus the selected model before registration. Startup timeout, early exit, malformed response, cancellation, or output overflow stop the entire process group and return typed body-free errors.
- On readiness, headless/TUI mutate only their in-memory config with provider ID `local-managed-<port>`, exact model ID, credentialless Chat Completions base URL, and process-only current selection. Config stores/defaults, provider catalogs, sessions' defaults, deleted providers, and future launches remain unchanged. The server stays pinned for the entire tool loop and stops on every run exit.
- `ca local serve` exposes the same foreground supervisor without entering a conversation, prints safe ready/provider facts, waits for Ctrl+C, then stops; it never daemonizes. Bounded logs are printed only on explicit status/failure and contain no raw model path or environment.

## Milestones

### M1 — GGUF store and explicit management

- Add failing tests for command routing/profile conflicts, exact file validation, header parsing, streaming bounds/hash, symlink/traversal/overwrite, no-write list, atomic failure, safe records/Debug, and import/remove previews.
- Implement profile paths, model store, import/list/inspect/remove, and exact confirmed download with loopback fixtures.

### M2 — Process supervision and provider registration

- Add a fake managed `llama-server` fixture installed through the EXT-088 test boundary or an injected reverified resolver.
- Characterize exact argv/environment/process group, loading 503 then ready/model discovery, bounded logs, health timeout/malformed/early-exit/cancellation, port ownership, and descendant cleanup.
- Implement foreground serve and process-only headless/TUI provider injection. Prove no config/session/default mutation and exact server shutdown after successful, failed, and cancelled turns.

### M3 — Delivery gates

- Run focused and full tests, formatting, strict Clippy, release build, help/version/profile/local smokes, and reviewed TUI states if a visible local-start surface is added.
- Exercise the released binary with a generated minimal GGUF and fake EXT-088 `llama-server` under a temporary home: import, inspect, start/loading/ready, one credentialless loopback request, exit cleanup, remove.
- Reconcile standard docs and archive only after actual evidence is recorded.

## Acceptance Criteria

- No model file/network/process action occurs without an exact explicit command and confirmation where persistent bytes change.
- Imports/downloads are regular non-symlink bounded GGUF files whose exact final bytes/hash match their record; failure/cancellation leaves no active record/staging file.
- Launch resolves and revalidates both model and EXT-088 runtime immediately before spawn; no PATH fallback or unverified executable is used.
- Fixed argv binds literal loopback, provider registration is process-only, and neither config/defaults nor deleted providers are restored.
- Health/model validation precedes provider traffic. Loading, ready, timeout, exit, malformed, and cancel states are typed, bounded, and response-body/path/secret free.
- Every server and descendant exits on ordinary completion, setup failure, cancellation, or process shutdown; no daemon or temporary directory survives.
- Focused tests, full tests, formatting, strict Clippy, release, and the disposable lifecycle pass without real models/runtimes/services/user state.

## Validation

Focused:

- `cargo test local_models --no-fail-fast`
- targeted main/parser/headless/TUI provider-injection tests

Completion:

- `cargo test --no-fail-fast`
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo build --release`
- release help/version/profile/local smokes

Manual/visual:

- Temporary home, generated bounded GGUF header, fake signed EXT-088 runtime, and fake loopback llama server only. No real microphone/GPU/model/provider/runtime is used.

Blocked checks:

- Real llama.cpp/GGUF performance, GPU memory, model quality, and public model downloads remain intentionally untested.

## Progress

- [x] Verify EXT-088, provider injection, process, sandbox, and oracle boundaries.
- [x] Add failing GGUF-store and CLI characterization tests.
- [x] Implement explicit import/download/list/inspect/remove.
- [x] Implement supervised local server and process-only provider registration.
- [x] Run repository-wide gates and disposable released lifecycle.
- [x] Reconcile harness documents and archive.

## Decisions

- Decision: managed local providers are process-only and require `--local-model` every run.
  Evidence: persisting or auto-restoring a provider would violate the program's deleted-provider and no-auto-start boundaries.
  Consequence: normal launches remain unchanged; selection is unmistakably explicit.
- Decision: start with fixed llama-server arguments and no arbitrary extra flags.
  Evidence: arbitrary flags can override host/model/port, widen file/network authority, or hide secrets.
  Consequence: tuning remains a later reviewed extension.
- Decision: GGUF import validates container identity and exact bytes but does not trust metadata as executable input.
  Evidence: the server, not `ca`, is the parser of record; deep tensor parsing would duplicate a large unsafe surface.
  Consequence: memory reporting is conservative and explicitly approximate.

## Surprises and Discoveries

- The current sandbox policy has only all-network or no-network modes. A managed loopback HTTP server cannot claim sandbox-enforced loopback-only networking without a separately characterized backend extension; fixed bind arguments and explicit launch authority remain mandatory and this limitation is visible.
- The initial command skeleton accepted `--profile bare` and had no persistence or process implementation; the focused red test failed exactly on the bare-profile invariant before the store was built.
- A truthful head/tail log capture needs overlap-aware reconstruction below its cap; concatenating both retained halves would duplicate bytes for outputs between 32 and 64 KiB. LOC-089 now reconstructs that middle range and emits an exact omission count only above 64 KiB.

## Recovery and Rollback

- Import/download staging uses one exact generation path below the model store and a Drop guard; the next locked mutation removes abandoned UUID staging entries.
- Local servers are process-owned, use their own process group, and are killed synchronously by guard drop; no persistent server registry exists.
- Persistent model removal first revalidates and renames one exact model/record pair within the store, saves state, and then deletes; failure restores where possible.

## Completion Record

Changed files:

- `src/local_models.rs`, `src/main.rs`, `src/runtime.rs`, `src/config.rs`, and `tests/cli.rs`.
- `docs/.INDEX.md`, `ARCHITECTURE.md`, `ENVIRONMENT.md`, `PARITY.md`, `TESTING.md`, `CONTINUITY.md`, and `NEXT_STEPS.md`.

Commands and results:

- Initial `cargo test local_models --no-fail-fast`: RED as intended because bare-profile persistent management was accepted by the command skeleton.
- Final focused local/model/parser/CLI suites: PASS, including 9 local model and supervisor tests.
- `cargo test --no-fail-fast`: PASS, 1075 unit tests and 35 CLI integration tests; one manual local Chromium test remains ignored by the ordinary gate.
- `cargo fmt --check`, strict Clippy, and release build: PASS.
- Released signed-runtime/GGUF walkthrough: PASS. One credentialless sandboxed loopback request returned `local-ok`; no config/session was created; the source GGUF remained; managed model/runtime trees were removed; and the owned server process exited.

Remaining risks:

- Real llama.cpp startup, GPU behavior, architecture-specific memory, large GGUF performance, and public model downloads remain intentionally untested.
- The current macOS sandbox grants broad network authority to the server process. Fixed literal-loopback binding and explicit exact launch are the reviewed boundary; sandbox-enforced outbound denial would require a separate design.

Harness updates:

- Standard architecture, environment, testing, parity, continuity, index, and next-step documents now record the new store, runtime consumer, process-only provider flow, validation evidence, and no parity claim.
