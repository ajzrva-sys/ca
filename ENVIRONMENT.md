# Environment and Runtime Contract

## Build Environment

- Rust 2021 workspace with the root `ca` package and
  `crates/codex-team` package under one `Cargo.lock` and `target/`.
- Root validation is pinned to Rust/Cargo 1.97.1 by
  `rust-toolchain.toml`; the project does not claim an MSRV until a separate
  lower-bound matrix establishes one.
- Verified targets in this checkout: macOS and FreeBSD terminals.
- Core stack: tokio, ratatui, crossterm, reqwest/rustls, serde.
- Build artifacts: `target/`; never treat them as source.

FreeBSD 15 amd64 builds natively with the packaged toolchain:

```sh
pkg install rust pkgconf
cargo build --release --locked
```

Full native tests and scheduled Git materialization also expect the packaged
`git` and `python3` tools. FreeBSD installs those executables under
`/usr/local/bin`; `ca` resolves scheduled Git from a closed absolute candidate
list and test fixtures record the exact Python path.

Core CLI/TUI, provider environment-variable authentication, Unix process
management, and descriptor-relative workspace materialization are supported.
The `required` process sandbox remains a macOS-only fail-closed mode; on
FreeBSD, command execution requires an explicit `policy-only` or `off` sandbox
selection. macOS Keychain credential storage, CoreAudio voice capture, and
LaunchAgent service installation remain unavailable, while their portable
environment-variable, manual, or foreground alternatives continue to work.
Signed runtime and self-update manifests identify this target as
`freebsd-x86-64`.

Use live tool output for compiler/tool versions. The exact root-validation pin
is authoritative; platform package-toolchain claims remain separately tested.

## Approved Command Toolchains

Required-sandbox command jobs resolve executables from canonical system roots
and validated Homebrew, `/usr/local`, selected Xcode, Cargo `bin`, and Rustup
`toolchains` roots. A trusted executable must be a canonical regular file,
owned by root or the current user, and not world-writable. AutoReview never
trusts an arbitrary inherited `PATH` directory. An explicitly approved
absolute executable outside those roots receives only a literal per-job read
rule.

On macOS, the ordinary sandbox permits read-only metadata traversal of `/etc`
so its symlink can reach the already-readable `/private/etc`; the closed
experiment profile does not receive that addition. Approved literal Cargo
stages receive read-only Cargo `registry` access and `CARGO_NET_OFFLINE=true`
when network was not requested and granted. Cargo configuration, credentials,
Git dependency caches, unrelated home files, and writable cache roots are not
added. Required sandboxing remains unavailable and fail closed on FreeBSD.

## Repository-Local Inputs

- `AGENTS.md` and `CLAUDE.md` are discovered from the working directory by `src/context.rs`.
- `.ca` files under `tests/` are the scripted test DSL.
- `oracles/` and `reference/` are read-only research trees and must not enter builds.
- `.hermes/plans/` contains historical agent plans; new plans belong under `docs/exec-plans/`.

## User Runtime State

Source currently uses these locations:

- config: `~/.config/ca/config.toml`;
- sessions: `~/.local/share/ca/sessions/`;
- user memory: `~/.local/share/ca/memory/user.json`;
- workspace memory: `~/.local/share/ca/memory/workspaces/<canonical-root-digest>.json`;
- reviewed Codex archive index/reviews/proposals: `~/.local/share/ca/learning/codex-archive-v1/`;
- schedules: `~/.local/share/ca/schedules/schedules.json`;
- local plugin registry: `~/.local/share/ca/plugins/plugins.json`;
- signed plugin catalog state and staged versions: `~/.local/share/ca/plugin-catalog/`;
- signed external-runtime catalog state and retained versions: `~/.local/share/ca/external-runtimes/`;
- managed GGUF state and exact model files: `~/.local/share/ca/local-models/`;
- global signed self-update state, staged releases, and rollback bytes: `~/.local/share/ca/self-update/`;
- direct provider credentials: `~/.local/share/ca/credentials/<provider>`;
- shell history: `~/.config/ca/shell_history`;

`--profile default` uses those exact legacy paths. A named `--profile NAME` uses `~/.config/ca/profiles/NAME.toml`, `~/.local/share/ca/profiles/NAME/sessions/`, memory below `~/.local/share/ca/profiles/NAME/memory/`, schedules below `~/.local/share/ca/profiles/NAME/schedules/`, plugins below `~/.local/share/ca/profiles/NAME/plugins/`, signed plugin catalogs below `~/.local/share/ca/profiles/NAME/plugin-catalog/`, external runtimes below `~/.local/share/ca/profiles/NAME/external-runtimes/`, managed GGUFs below `~/.local/share/ca/profiles/NAME/local-models/`, and provider credentials below `~/.local/share/ca/profiles/NAME/credentials/`; shell history remains shared. A missing named config is not created until a successful mutation. `--profile bare` reads the normal config path without creating it when absent and does not persist sessions, memory mutations, config changes, credentials, schedules, plugins, catalog state, managed runtimes, or managed GGUFs.

Local plugin manifests remain at user-selected source paths. The registry stores their exact canonical locations and content digests but never copies or deletes bundle content. Enabled bundles can contribute only existing skill roots, MCP definitions, hook definitions, workflow files, and aliases to built-in themes. Manifest MCP environment/header values must be exact environment references; resolved values remain runtime-only.

Portable skills are discovered from repository-ancestor `.agents/skills` directories (stopping at the primary root), `~/.agents/skills`, `/etc/codex/skills`, profile-managed storage, and retained legacy/configured roots. Exact enable records live at the profile data path `skills/state.json` with owner-only atomic writes. Manager list/show and unconfirmed preview commands do not create that file. `allowed-tools`, scripts, and dependency declarations are inert metadata and confer no runtime authority.

Signed catalog snapshots and package files are local inputs obtained out of band; `ca` has no catalog URL, marketplace browser, automatic download, or built-in trust-on-first-use root. Trust keys are explicitly pinned per profile. Verified packages are copied into private profile data, remain disabled after install/update/rollback, and retain old versions for explicit rollback.

`ca runtime` is a separate signed external-runtime boundary. It has no built-in source or trust root. `trust` explicitly pins one exact HTTPS base path, or literal-loopback HTTP for a local fixture, plus one local Ed25519 public key; `sync` explicitly fetches only its `catalog.json`. Signed entries contain direct relative files rather than archives or install scripts. `preview`, `list`, `path`, `rollback`, and `remove` are offline; install/update stream only declared files after exact confirmation and retain old versions. Managed bytes are owner-only and inert by default. Only an explicit `ca local serve` or standard run with both `--local-model` and `--local-runtime` resolves and executes the exact named `llama-server`; LSP, tmux, browser, voice, and discovery still do not consult this store. No command uses a proxy, follows redirects, accepts URL credentials/query values, or performs a background check/download.

`ca local` has no built-in model catalog and never scans, imports, downloads, starts, or persists a provider automatically. Import preserves the user source; download requires an exact URL, SHA-256, byte size, license label, and matching confirmation. A standard managed local run is credentialless, loopback-only at the provider boundary, process-owned, and no-session; it reads defaults without creating a missing config, mutates only an in-memory provider/default, and kills the owned server group on exit.

`ca update` is global rather than profile-scoped because it targets the exact invoking executable. It has no built-in release source or trust root and performs no background work. Trust, channel, pin, and status are local-only; check fetches only the explicitly pinned signed manifest; download stages only one confirmed current-target artifact; install and rollback mutate only the invoking copied/current executable after exact version confirmation. Repository tests and walkthroughs use copied binaries under temporary homes and never replace an installed binary.

Memory recall and proposal generation are independently disabled by default for user and workspace scopes. Opening `/memory` or preparing a request does not create files. Enabled recall is bounded to 200 lines and 25 KiB per scope; generated records stay process-only until explicit acceptance. Memory files contain no credentials or runtime authority and use stable canonical-workspace digests rather than raw path components.

Goal-session defaults are finite and profile-configurable under `[goals]`: `max_continuations = 12`, `max_tokens = 100000`, and `max_working_minutes = 30`. Per-run `/goal` flags may lower those resolved defaults but never raise them. Goal evaluation receives no executable tools or runtime credentials; active goals restored from disk reopen paused and require explicit resume.

These locations are derived by `RuntimePaths` and injected at startup. If `HOME` is unavailable, startup fails deterministically instead of using cwd or a developer-specific fallback.
- custom agents: project and user `.ca/agents/*.md` directories;
- project instructions: nearest `AGENTS.md` or `CLAUDE.md`.

Verify source before relying on a path. Tests must redirect persistence to a temporary root rather than mutate these locations.

## Reviewed Codex Archive Sources

`ca learn source preview|add|list|remove` manages explicit profile-scoped
read-only archive roots. Registration requires the exact preview digest. Each
scan is limited to 10,000 files and 1 GiB total, 64 MiB per regular no-follow
file, 1 MiB per streamed JSONL record, depth 64, and 8 KiB per sanitized
message. The index file and singleton lock are owner-only; source files are
never changed or copied. `/import codex` remains the separate small-history
session import and is not used by archive learning.

`ca learn review show` may read the configured priced route to bind provider,
model, planned calls, and aggregate estimated cost into an evidence digest; it
does not send traffic. `review approve DIGEST` is the only provider boundary.
Generated proposals remain inert until an exact `proposal apply DIGEST` (with
optional repeatable `--run` or `--item` selectors) or rejection. Item selectors
are content-bound memory-operation or skill-package digests shown by
`proposal show`. Source registration, service install, provider processing,
proposal application, and installed-binary replacement are distinct
confirmations.

On macOS, `ca learn service preview|install|status|uninstall` manages a
secret-free per-profile LaunchAgent after exact plist-digest confirmation. It
uses `~/.local/bin/ca` directly, without a shell, every 900 seconds and at
login. The background child clears inherited environment values except HOME
and its internal marker, denies networking, writes only the archive index, and
reads only registered sources plus system/runtime files. Service lifecycle and
background scanning fail closed on other platforms; foreground preview/sync
remain available.

## Local Service Transport

`ca serve` and `ca serve --stdio` use bounded JSONL stdin/stdout. `ca serve --listen 127.0.0.1:PORT` enables the same `ca.serve.v1` protocol over one authenticated loopback WebSocket client; `ca attach ws://127.0.0.1:PORT` is its bounded JSONL bridge. Only literal loopback IPs are accepted. For remote use, create an SSH tunnel outside `ca` and attach to the tunnel's local endpoint; non-loopback listeners and TLS termination are unsupported.

WebSocket authentication requires a value from `--auth-file PATH` or `--auth-env NAME`; no CLI token-value option exists. On Unix, token files must be current-user-owned, non-symlink regular files with group/other access disabled. Token values are 32–1024 bytes of visible ASCII and never enter Debug output, service events, or diagnostics. Service/attach sessions remain process-only and never use saved-session storage.

## Provider Configuration

`src/config.rs` supports named providers with model, base URL, API-key environment variable, model lists, and a defaulted wire protocol. Omitting `protocol` preserves the historical Chat Completions behavior without rewriting TOML; accepted explicit spellings are `protocol = "chat_completions"`, opt-in `protocol = "openai_responses"`, and opt-in `protocol = "anthropic_messages"`. Responses uses `/responses` with conditional bearer auth. Anthropic uses `/messages`, `anthropic-version: 2023-06-01`, conditional `x-api-key`, a fixed bounded 16,384 output-token request limit, and manual local text/client-tool history; it does not persist thinking signatures. Hosted tools, server conversation IDs, cloud-host variants, prompt-cache policy, and multimodal Items are not implied. `ConfigStore` retains the raw TOML document while resolving `${NAME}` provider keys only in memory; approval and default-model patches atomically preserve the raw placeholders. The source contains historical DeepSeek and Ollama defaults, while project notes identify MiMo as the current operational provider. This is unresolved configuration drift, not permission to rewrite defaults.

`/provider` always presents four connection-first rows. Xiaomi MiMo uses the published MiMoCode X25519 handoff and selects `mimo-v2.5-pro`; OpenAI Codex uses browser PKCE on macOS or the documented device-code page on FreeBSD/headless and selects `gpt-5.6-sol`; DeepSeek and direct OpenAI accept masked bearer keys and select `deepseek-v4-pro` and `gpt-5.6-sol` respectively. Direct OpenAI is a distinct Responses-protocol provider and never consumes ChatGPT subscription credentials. Setup performs one isolated request, persists and switches only on success, rolls back authentication failures, and retains non-authentication failures for later repair without switching.

New direct credentials are represented in TOML only by `credential://ca/provider/<provider>` for the default profile or `credential://ca/profile/<profile>/provider/<provider>` for a named profile. `${ENV_VAR}` and legacy `keychain://ca/...` references remain readable. All supported persistent profiles use their active profile data directory's `credentials/` tree: every directory is current-user-owned mode `0700`; files are current-user-owned regular files mode `0600`; reads are bounded and no-follow/close-on-exec; replacement is atomic. On macOS only, a missing private provider credential is imported once from the matching legacy `ca.provider-api-key` Keychain item; the import must commit before the value is returned, later loads never consult Keychain while the file exists, and the legacy item is retained for rollback. This does not apply to Codex OAuth or the dedicated email-gateway Keychain. Bare profiles warn before authentication, retain credentials only for the process, and cannot persist or become the default.

`/provider` and `/provider setup` expose `Shift+O` as an explicit OpenAI template. As verified against official OpenAI documentation on 2026-08-12, it opens the existing editable masked form with these safe values and makes no config, credential, or network change until Enter saves:

```toml
[providers.openai]
base_url = "https://api.openai.com/v1"
api_key = "${OPENAI_API_KEY}"
protocol = "openai_responses"
models = ["gpt-5.6-sol"]
```

Saving from provider setup adds only that provider and leaves the current/default provider unchanged until the existing tested finish chooser is used. A configured `openai` provider is never overwritten by the template. Generic Add Provider still omits `protocol` and remains Chat Completions. If `OPENAI_API_KEY` is absent, discovery/checking reports the missing environment locally before provider traffic; only an explicit provider test may send the documented isolated request.

`Shift+P` from `/provider` or `/provider setup`, and `/provider templates [filter]`, open a compiled snapshot of all 40 inference-provider rows in the Hermes Agent provider guide as audited on 2026-08-12. The catalog is local and performs no runtime fetch. It classifies 14 rows as ready: 11 Chat Completions rows (OpenRouter, Arcee AI, both Alibaba/Qwen API-key routes, Xiaomi MiMo, Tencent TokenHub, DeepSeek, Ollama Cloud, StepFun, LM Studio, and Custom Endpoint), two Responses rows (Actual Computer and direct OpenAI), and one Messages row (Anthropic API key). Selecting a ready row opens the same editable masked provider form and only explicit Save writes it.

The other 26 rows remain searchable but show a typed integration requirement and cannot open a misleading editor. They cover OAuth/refresh flows, AWS/Vertex/Azure auth, Copilot ACP, per-model routing, and providers whose tool continuations require thought signatures, alternate reasoning schemas, or request controls that `ca` does not yet retain or replay. Examples include Gemini 3, MiniMax, Fireworks/Novita/Kimi, z.ai, NVIDIA, and xAI, plus unproven gateway variants. “Ready” therefore means the current full agent loop is represented by an existing adapter, not merely that a one-shot HTTP request resembles OpenAI.

Continuation behavior is activated only for the canonical provider IDs `openai`, `deepseek`, and `mimo`; custom aliases retain generic adapter behavior. The opaque state is bound to the actual provider ID, protocol, and normalized endpoint SHA-256 chosen by the runtime client, so it cannot cross a provider, protocol, endpoint, or fallback route. OpenAI requests `reasoning.encrypted_content` and locally replays the validated completed output sequence without `previous_response_id`; DeepSeek and MiMo replay the exact returned `reasoning_content` on the assistant tool-call message. Direct sessions preserve this state, while portable exports/imports, checkpoint prompts, request snapshots, Debug, and the UI omit it. Consequently, portable or legacy transcript import is readable but is not continuation-equivalent to a direct saved session for these providers.

Rules:

- never read, display, record, or commit secret values;
- refer only to environment-variable names;
- use a fake local transport for tests;
- do not make live-provider tests part of `cargo test`;
- provider-specific behavior belongs behind explicit configuration/adapters.
- unsupported protocol names fail during config decoding before provider traffic or secret resolution.

Image input is opt-in for an exact model profile. New configurations declare modalities explicitly:

```toml
[model_profiles."provider/model"]
input_modalities = ["text", "image"]
```

Legacy `supports_images = true` remains readable. Missing image capability is text-only; model names never imply support. Accepted clipboard and `view_image` inputs are decoded, bounded, metadata-stripped, and normalized to PNG in process memory. Their bytes are never persisted or included in debug/snapshot/structured output; sessions retain only an unavailable-safe MIME/dimension marker.

Environment-variable names referenced by source include provider API-key variables and image-generation configuration. Inspect source for the exact current list without printing values.

ChatGPT subscription authentication for the exact `openai-codex` provider uses the documented Codex file cache at `$CODEX_HOME/auth.json` (default `~/.codex/auth.json`). macOS can populate it through browser PKCE; FreeBSD/headless uses OpenAI's bounded device-code flow and creates a missing cache mode `0600`. Existing unrelated JSON fields survive token updates. The cache must be a regular, owner-only file owned by the current user, its values must never be printed or logged, and it must never authenticate another provider. Copying it to a trusted headless host follows OpenAI's documented fallback and the file must be treated like a password. Browser cookies and account-password handling remain forbidden.

Optional interactive fallback rules use exact configured pairs and remain profile-scoped:

```toml
[[fallbacks]]
provider = "primary"
model = "primary-model"
fallback_provider = "backup"
fallback_model = "backup-model"
causes = ["connect", "timeout", "rate_limited", "server_error"]
```

Omitting `causes` enables those same four closed causes. Rules never contain credentials or endpoints and are rejected if a pair is missing, duplicated, self-referential, or has an empty/unknown cause set. Fallback is interactive-main-turn only, before usable output, one route per turn, and never changes the configured default; exact-provider automation does not inherit it.

Voice input is disabled by default. It may use one existing configured provider/model:

```toml
[voice]
enabled = true
max_seconds = 60

[voice.backend]
type = "provider"
provider = "ollama"
model = "whisper-model"
timeout_secs = 30
```

or one existing absolute local executable. The argument vector must contain exactly one literal `{audio}` placeholder and is never evaluated by a shell:

```toml
[voice]
enabled = true
max_seconds = 60

[voice.backend]
type = "local"
executable = "/absolute/path/to/transcriber"
args = ["--audio", "{audio}"]
timeout_secs = 30
```

`max_seconds` is restricted to 1–300 and transcription timeouts to 1–120 seconds. `/voice` records from the macOS default input only after Space, retains a bounded mono WAV only for the active generation, and inserts successful text as an editable unsent draft. The local backend runs in the required network-denied sandbox with access only to configured workspace roots and the temporary WAV directory. The provider backend makes one bounded multipart request to that provider's `/audio/transcriptions` endpoint; credentials remain at the runtime boundary. Audio is never written to configuration or sessions.

## MCP Configuration

Omitting `transport` preserves the existing local stdio server shape. Streamable HTTP is explicit and retains header credential references in raw/runtime management configuration:

```toml
[mcp.servers.remote]
transport = "streamable_http"
url = "https://mcp.example.invalid/rpc"
headers = { Authorization = "${MCP_REMOTE_AUTHORIZATION}" }
network = true
```

Streamable HTTP requires the same process/server network grant as networked stdio, even on loopback. Plain HTTP accepts only a literal loopback IP; URLs may not contain credentials, queries, or fragments. Header values must be exact uppercase environment references and cannot override protocol, forwarding, cookie, proxy, content, connection, or session headers. The client ignores ambient proxies and redirects, applies bounded deadlines, and never surfaces response bodies or authentication challenges. OAuth is not implied by this reference-only header path.

OAuth is an explicit alternative to an `Authorization` header:

```toml
[mcp.servers.secure]
transport = "streamable_http"
url = "https://mcp.example.invalid/rpc"
network = true

[mcp.servers.secure.oauth]
enabled = true
issuer = "https://login.example.invalid/tenant"
client_id = "ca-client"
scopes = ["tools.read"]
```

`/mcp connect secure` starts browser authorization with PKCE; `/mcp refresh secure` and `/mcp disconnect secure` operate on that exact server. Device authorization requires `flow = "device"`, an advertised device endpoint, and a pre-registered public `client_id`. Optional confidential browser clients use an exact environment reference such as `client_secret = "${MCP_CLIENT_SECRET}"`; the value is resolved only inside the explicit authorization worker.

MCP OAuth tokens use macOS generic-password service `ca.mcp-oauth`. Default-profile accounts are `server/<server>` and named-profile accounts are `profile/<profile>/server/<server>`; bare uses a process-only store. Do not inspect, migrate, or delete these items outside the exact manager action. Cross-origin authorization discovery requires an exact configured issuer pin, remote endpoints require HTTPS, and no browser/device flow starts from ordinary catalog loading.

Configured MCP servers are not started merely because a provider request is prepared. Ordinary main/headless requests advertise `search_mcp_tools`; executing it explicitly discovers an exact server or a bounded eligible subset and activates only matching schemas for later turns. Use an exact `server` argument when more than eight servers are configured. Activations are process-only and disappear on catalog changes, reconnect/disconnect, or exit. `/mcp` remains the explicit user management/status path and may inspect the full catalog.

## LSP Configuration

Language servers are disabled by default and never auto-detected or installed. Each enabled record names one existing executable, its arguments, language identity, accepted file extensions, and a bounded request timeout:

```toml
[lsp.servers.rust]
command = "rust-analyzer"
args = []
language_id = "rust"
extensions = ["rs"]
enabled = true
timeout_secs = 10
```

The six `lsp_*` tools require the exact server ID and a workspace file. Processes start only on an explicit tool call, run with network denied under the effective sandbox, receive a cleared allowlisted environment, and remain process-local. Bare, review, plan, checkpoint, scripted, and subagent contexts do not receive LSP tools. Returned non-file or external-workspace URIs fail the whole request; results expose only workspace-relative paths. `ca` does not download servers, execute server commands, apply workspace edits, or persist document content.

## PDF Reading

`read_pdf` is a self-contained read-only tool backed by pinned pure-Rust `pdf-extract`/`lopdf` dependencies. It invokes no local PDF executable and performs no network access. It accepts only regular workspace `.pdf` files up to 32 MiB, rejects encrypted documents, parses on a blocking worker, extracts at most 20 selected pages and 56 KiB of text, and returns at most 64 KiB including total page/file and next-range metadata. Embedded files, annotations, actions, JavaScript, passwords, OCR, and image rendering are outside this boundary.

## External Commands and Services

Some tools or slash commands invoke external programs/services, including shell commands, Git, GitHub CLI, clipboard utilities, web endpoints, image generation, and messaging helpers. Availability is optional and must be detected at the boundary.

Email tools are disabled by default. Enabling them requires an already-installed absolute Himalaya executable compatible with v2.0.x, an optional absolute config path, and explicit non-secret account aliases/sender identities. Himalaya alone owns mailbox credentials. ca never installs Himalaya, imports credentials, reads its config contents, or validates against a live mailbox during repository testing.

The separate EML-101 gateway is also disabled by default and must use a dedicated account ID/address that is absent from the Himalaya mappings. Its app password exists only in the profile/account-scoped `ca.email.gateway` macOS Keychain item referenced by `keychain://ca/email-gateway/<account>`; TOML, environment variables, LaunchAgent XML, Debug, and output never contain it. IMAP and SMTP endpoints require either implicit TLS or required STARTTLS with platform certificate verification. Pinned `async-imap`, `mail-parser`, `tokio-rustls`, and `rustls-platform-verifier` dependencies provide the transport/parser boundary; ca invokes no shell, mail client, or credential helper.

`ca email doctor` is offline and config-only. `--connect` explicitly reads the gateway Keychain item and tests IMAP plus SMTP TLS/auth without sending. `ca email gateway once|daemon` may read post-bootstrap allowlisted mail and append only `\Draft` messages. `ca email service preview` is deterministic and mutation-free; install/uninstall require the exact digest and tests use fake `launchctl`. Repository tests must never use a real gateway account, Keychain item, provider, or LaunchAgent.

The opt-in `browser` tool discovers an existing Google Chrome or Chromium executable; `ca` never installs or updates one. It starts headless with a new temporary user-data directory, cleared ambient environment, background services disabled, and a literal-loopback random CDP port. The user profile, cookies, password store, extensions, and browser sessions are never loaded. Navigation and downloads require their own explicit network authority; external network is CDP-blocked for all other actions. DOM text is terminal-sanitized and capped at 64 KiB, screenshots are metadata-stripped normalized PNGs written only to an approved new workspace path, downloads are capped at 16 MiB and cannot overwrite, and the ten-minute owner-bound process group/profile are removed on close, run cleanup, or process shutdown.

Voice capture uses the existing macOS CoreAudio runtime through `cpal`; microphone permission is requested by the operating system only when recording is explicitly started. A local voice backend is optional and must already be installed at its configured absolute path. `ca` does not install a transcriber or retain captured audio.

Tests must not assume:

- network access;
- authenticated `gh`;
- a clipboard service;
- MiMo/DeepSeek/OpenAI/Ollama availability;
- user-specific helper binaries;
- Chrome or Chromium unless the browser walkthrough is explicitly selected;
- microphone access or a local transcription executable unless the voice walkthrough explicitly starts recording;
- a Git repository with mutable history.

## Scheduled Automation

V1 definitions remain in the profile schedule directory's `schedules.json`. V2 definitions use `schedules-v2.json`; immutable receipts use `schedule-runs-v2/`; leases and WSP roots use profile-owned `schedule-private-v2/`; verified recovery archives and local outputs use separate profile schedule subdirectories. All are owner-only and bare profiles cannot schedule.

`ca schedule service preview` is local, deterministic, and mutation-free. It emits a secret-free user LaunchAgent plist on stdout and its exact confirmation digest on stderr. `install` and `uninstall` mutate only the exact per-profile plist after `--confirm DIGEST`; tests must pass an absolute fake `launchctl` and must never load a real user service. The service runs the absolute revalidated binary with `schedule --profile NAME tick --drain`, not a shell.

V2 isolated jobs require `/usr/bin/git`, a clean canonical source repository, one exact full commit ID, and the existing WSP private-root capacity. Script-only jobs require an absolute canonical executable and its SHA-256 in the confirmed definition. No script-only path constructs a provider client or inherits provider credentials.

V2 `email_draft` delivery is available only when its confirmed definition contains the exact dedicated gateway account and sorted unique canonical recipients. Delivery composes and appends one plain-text Drafts message; it never invokes EML-100 or SMTP send.

## Installation

The repository build is `cargo build --release`. Installing by copying `target/release/ca` to `~/.local/bin/ca` mutates user state and is not a default validation step. Perform it only when explicitly requested, after release validation.

## Safe Inspection

Safe by default:

```bash
cargo metadata --no-deps --format-version 1
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release
./target/release/ca --help
```

`./target/release/ca --help` and `./target/release/ca --version` are local exits and do not load runtime state or contact a provider.

Potentially stateful or external; require task-specific scope:

```bash
cargo run
./target/release/ca --test tests/basic.ca
cp target/release/ca ~/.local/bin/ca
```

The interactive binary can create/read runtime config, sessions, shell history, project files, and provider connections.
