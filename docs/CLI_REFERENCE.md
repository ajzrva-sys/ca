# CLI reference

Captured from the signed all-feature release build on 2026-10-06.
Run `ca --help` for the installed version and `ca COMMAND --help` for each
command family. This page is generated help, not a substitute for the usage guides.

```text
Cursor-compatible terminal coding agent

Usage: ca [OPTIONS] [PROMPT_OR_@FILE]...

Arguments:
  [PROMPT_OR_@FILE]...  One-shot prompt and optional @path attachments

Options:
      --min-version <SEMVER>
          Fail before initialization unless this CA version satisfies the strict SemVer minimum
  -c, --continue
          Continue the most recently updated session
  -r, --resume-list
          Open the saved-session picker
      --dashboard
          Open the agent dashboard
      --name <NAME>
          Set the session display name
  -p, --prompt <PROMPT>
          Run one prompt without opening the TUI
  -s, --session <ID>
          Resume by ses_ identifier, UUID, or legacy storage key
      --test <SCRIPT_FILE>
          Run a scripted .ca test case
      --output-format <OUTPUT_FORMAT>
          Select human text, aggregate JSON, or streaming JSONL output [default: text] [possible values: text, json, jsonl]
      --show-thinking
          Include provider reasoning events when supported
      --auto-review
          Automatically execute read-only tools in headless mode using CA-local safe-tool evaluation (not a remote classifier)
      --yolo
          Bypass interactive approval prompts (boundaries still apply; exclusive with --auto-review)
      --allow-network
          Permit approved network-capable tools in headless mode
      --trace
          Persist a redacted provider exchange trace for this run
      --output-last-message <PATH>
          Atomically write the terminal assistant message to a file
      --check
          Run a fresh-context verification pass after a successful headless task
      --check-repair
          Permit the verification pass to propose repairs through normal tool policy
      --check-converge
          Require two explicitly configured isolated reviewers to pass
      --check-converge-unpriced
          Acknowledge unknown pricing for this one convergent review run
      --output-schema <PATH>
          Require the headless result to match a bounded local JSON Schema
      --skill <ID>
          Activate an exact filesystem skill ID (repeatable)
      --tools <TOOL[,TOOL]>
          Replace the default model-visible tools (repeatable or comma-delimited)
      --include-tools <TOOL[,TOOL]>
          Add model-visible tools after the default or replacement set
      --exclude-tools <TOOL[,TOOL]>
          Remove model-visible tools after replacement and inclusion
      --list-models
          List configured models and exit
      --doctor
          Check local ca readiness and exit without contacting providers
      --connect
          Include one isolated provider request per configured provider
      --provider-contract <PROVIDER_CONTRACTS>
          Verify one first-party provider contract with at most two no-retry requests (repeatable) [possible values: openai, mimo, deepseek]
      --no-session
          Keep this run and its branches in memory without reading or writing saved sessions
      --single-turn
          Do not wait for automatic background-job completion follow-ups
      --keep-awake
          Keep the machine awake only while an interactive turn or model-owned job is active
      --no-keep-awake
          Disable active-work sleep inhibition for this process
      --conversation-history-file <FILE>
          Seed this process from a bounded ca.session-export.v1 file without importing it
      --exclude-workspace-context
          Omit automatically sourced workspace context for this process
      --new-session-id <UUID>
          Select the UUID for a fresh persistent interactive session
      --with-diffs
          Attach a bounded host-captured local Git snapshot to the first turn
      --printenv
          Emit redacted child-environment changes after commands in structured headless output
      --profile <NAME>
          Select a whole-file run profile (`default`, `bare`, or a named profile)
      --workspace <PATH_OR_NAME>
          Start from a saved workspace name or directory path
      --add-dir <PATH>
          Add a process-only workspace root (repeatable)
      --plugin-dir <DIR>
          Load a validated local plugin directory for this process only (repeatable)
      --direnv
          Evaluate the primary workspace's .envrc for this trusted process only
      --mode <MODE>
          Start in plan or ask mode for this process only [possible values: plan, ask]
      --plan
          Start in plan mode for this process only
      --sandbox <MODE>
          Override the configured sandbox for this process only [possible values: required, policy-only, off]
      --trust
          Trust the fresh workspace for this headless process only
      --worktree[=<NAME>]
          Create and launch in a registered worktree (optional name requires '=')
      --worktree-base <REF>
          Select the Git base for a launch-created worktree
      --skip-worktree-setup
          Skip repository-defined setup for the launch-created worktree
      --worktree-setup-digest <SHA256>
          Authorize the exact headless worktree setup preview
      --local-model <MODEL_ID>
          Start one exact managed GGUF for this process-only run
      --local-runtime <RUNTIME_ID>
          Select the exact managed runtime containing the llama-server entrypoint
      --local-context-size <LOCAL_CONTEXT_SIZE>
          Context size for an explicitly managed local run [default: 4096]
      --local-startup-timeout <LOCAL_STARTUP_TIMEOUT>
          Maximum seconds to wait for an explicitly managed local server [default: 300]
  -h, --help
          Print help
  -V, --version
          Print version

Commands:
  dashboard                           Open the agent dashboard
  serve [--stdio | --listen IP:PORT]  Run the versioned JSONL service
  acp                                 Run the stable ACP v1 stdio agent
  attach ws://IP:PORT                 Bridge JSONL to an authenticated loopback service
  schedule <COMMAND>                  Manage explicit profile-scoped scheduled runs
  automation <COMMAND>                Manage confirmed local automations and results
  automation-engine <COMMAND>         Operate durable typed automations without a provider
  role-run --profile NAME             Run one restricted orchestrator role
  learning-bridge <COMMAND>           Exchange sanitized enrolled learning evidence
  email <COMMAND>                     Manage the dedicated draft-only email gateway
  skill <COMMAND>                     Inspect and manage portable Agent Skills
  learn <COMMAND>                     Operate continuous local Dream/Distill learning
  audit                               Audit the local agent harness without execution
  support-bundle                      Create a private metadata-only support archive
  config explain [SELECTOR]           Explain setting origins without showing values
  import setup claude                 Preview/apply inert Claude setup assets
  plugin <COMMAND>                    Manage explicit local plugin bundles
  pricing <COMMAND>                   Inspect or refresh official route pricing
  runtime <COMMAND>                   Manage explicitly selected signed runtimes
  local <COMMAND>                     Manage explicit GGUF models and local runs
  update <COMMAND>                    Explicitly verify and install signed ca releases
  repo info|status|worktree-setup     Inspect local repository state and setup
  shell-integration zsh               Print zsh completion and terminal-key setup
  install-shell-integration           Install the managed zsh integration block
  uninstall-shell-integration         Remove only the managed zsh integration block

Attachments may be supplied as @path positional arguments.
```

## Command families

### `ca serve`

```text
Run the versioned JSONL service over stdio or authenticated loopback WebSocket

Usage: ca [OPTIONS]

Options:
      --stdio             Use bounded JSONL operations on stdin and events on stdout
      --listen <IP:PORT>  Listen for one authenticated client on a literal loopback IP and port
      --auth-file <PATH>  Read the WebSocket bearer token from an owner-only regular file
      --auth-env <NAME>   Read the WebSocket bearer token from the named environment variable
      --profile <NAME>    Select a whole-file run profile (`default`, `bare`, or a named profile)
      --allow-network     Permit a session to request client-approved network-capable tools
      --trace             Persist a redacted provider exchange trace for this service run
  -h, --help              Print help
```

### `ca acp`

```text
Run CA as a stable ACP v1 agent over newline-delimited JSON-RPC stdio

Usage: ca acp [OPTIONS]

Options:
      --profile <NAME>  Select a whole-file run profile (`default`, `bare`, or a named profile)
      --allow-network   Permit client-approved network-capable tools for this ACP process
  -h, --help            Print help
```

### `ca attach`

```text
Bridge local JSONL stdin/stdout to an authenticated loopback ca service

Usage: ca [OPTIONS] <ws://IP:PORT[/PATH]>

Arguments:
  <ws://IP:PORT[/PATH]>  Exact literal-loopback WebSocket endpoint

Options:
      --auth-file <PATH>  Read the bearer token from an owner-only regular file
      --auth-env <NAME>   Read the bearer token from the named environment variable
  -h, --help              Print help
```

### `ca schedule`

```text
Manage explicit profile-scoped scheduled runs

Usage: ca [OPTIONS] <COMMAND>

Commands:
  list     List safe schedule metadata
  add      Validate and add a local TOML or JSON schedule, disabled
  update   Replace a V2 schedule after exact digest confirmation; leaves it paused
  enable   Enable a schedule and compute its next invocation
  disable  Disable a schedule
  pause    V2 spelling for disable
  resume   V2 spelling for enable
  remove   Permanently remove one schedule
  run      Run one schedule immediately without enabling it
  tick     Claim at most one due invocation
  history  Inspect immutable V2 run receipts without executing anything
  doctor   Validate V2 definitions, journals, and recovery state offline
  service  Preview or explicitly manage the per-profile user LaunchAgent
  daemon   Poll for due invocations until interrupted; installs no system service
  help     Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
      --allow-network   Upper bound for schedule-declared provider tool network access
      --allow-webhooks  Upper bound for signed webhook delivery
  -h, --help            Print help
```

### `ca automation`

```text
Manage confirmed local automations and their run history

Usage: ca automation [OPTIONS] <COMMAND>

Commands:
  status     Inspect the durable native execution, without rerunning effects
  cancel     Cancel the run's owned worker; preserve completed effects
  reconcile  Reconcile persisted outcomes before considering continuation
  list       List automations without exposing prompt bodies
  inspect    Inspect one automation definition
  history    Inspect local run receipts
  doctor     Validate stores, routes, prices, and service readiness without mutation
  preview    Normalize a draft and print its exact confirmation digest
  create     Create and enable an automation after exact preview confirmation
  update     Replace an automation after exact preview confirmation
  pause
  resume
  run        Run one occurrence immediately without changing its recurrence
  delete     Cancel-first deletion requiring the exact automation ID
  service    Preview or explicitly manage the existing scheduler LaunchAgent
  help       Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
      --output <OUTPUT>  [possible values: text, json]
      --json
  -h, --help             Print help
```

### `ca automation-engine`

```text
Preview and operate one durable typed automation without a provider

Usage: ca automation-engine [OPTIONS] <COMMAND>

Commands:
  preview
  start
  status
  cancel
  reconcile
  doctor
  config-migration
  help              Print this message or the help of the given subcommand(s)

Options:
      --profile <PROFILE>
  -h, --help               Print help
```

### `ca role-run`

```text
Run one restricted, process-scoped orchestrator role

Usage: ca [OPTIONS] --profile <NAME>

Options:
      --profile <NAME>  Select the required nonreserved named profile
      --preflight       Validate the profile offline and emit one readiness object
  -h, --help            Print help
```

### `ca learning-bridge`

```text
Exchange sanitized learning evidence with an enrolled local supervisor

Usage: ca <COMMAND>

Commands:
  preflight    Inspect one named evidence profile without creating state or contacting a provider
  enroll       Bind one named profile to an exact supervisor enrollment
  export       Export queued, sanitized learning intents without mutation or provider traffic
  acknowledge  Acknowledge one exact intent only after it is durably imported
  complete     Publish one replay-approved artifact or record one terminal rejection
  help         Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `ca email`

```text
Manage the dedicated draft-only email gateway

Usage: ca [OPTIONS] <COMMAND>

Commands:
  doctor   Validate dedicated gateway configuration without network access by default
  gateway  Poll the dedicated inbox and append human-review drafts only
  service  Preview or explicitly manage the per-profile gateway LaunchAgent
  help     Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
  -h, --help            Print help
```

### `ca skill`

```text
Inspect and manage portable Agent Skills

Usage: ca [OPTIONS] <COMMAND>

Commands:
  list
  show
  enable
  disable
  install
  import
  new
  help     Print this message or the help of the given subcommand(s)

Options:
      --profile <PROFILE>  [default: default]
  -h, --help               Print help
```

### `ca learn`

```text
Operate continuous local Dream/Distill learning

Usage: ca [OPTIONS] <COMMAND>

Commands:
  source
  sync      Scan registered sources locally. This never contacts a provider
  status    Show local index and review status without transcript text
  run       Run one due harvest and one supervised learning step
  pause     Pause the autonomous learning supervisor
  resume    Resume the autonomous learning supervisor after health checks
  review    Inspect or authorize an exact evidence revision
  proposal  Inspect and selectively apply generated memory/skill proposals
  service   Preview or manage the unified autonomous learning service
  help      Print this message or the help of the given subcommand(s)

Options:
      --profile <PROFILE>  [default: default]
  -h, --help               Print help
```

### `ca audit`

```text
Audit local agent-harness surfaces without executing them

Usage: ca [OPTIONS]

Options:
      --profile <NAME>  Select the active profile whose configured harness surfaces are audited
  -h, --help            Print help
```

### `ca support-bundle`

```text
Create a private, metadata-only local support archive

Usage: ca support-bundle [OPTIONS]

Options:
      --output <FILE>     Write the archive to this new path instead of the private profile default
      --since <DURATION>  Retain runtime metadata no older than this duration (for example 2h or 7d)
      --json              Emit the versioned result as JSON
      --profile <NAME>    Select the whole-file profile to diagnose
  -h, --help              Print help
```

### `ca config explain`

```text
Explain where effective settings came from without showing values

Usage: ca config explain [OPTIONS] [SELECTOR]

Arguments:
  [SELECTOR]

Options:
      --json
      --profile <NAME>
  -h, --help            Print help
```

### `ca import setup claude`

```text
Usage: ca import setup claude [OPTIONS] --path <DIR> --scope <SCOPE>

Options:
      --path <DIR>
      --scope <SCOPE>          [possible values: user, project]
      --json
      --apply-digest <SHA256>
  -h, --help                   Print help
```

### `ca plugin`

```text
Manage explicit local plugin bundles

Usage: ca plugin [OPTIONS] <COMMAND>

Commands:
  list      List safe plugin metadata
  add       Validate and add a local plugin manifest, disabled
  show      Show safe contributions, capabilities, and collisions
  enable    Enable an unchanged collision-free bundle
  disable   Disable a plugin bundle
  remove    Forget a disabled plugin without deleting its source
  workflow  Resolve an enabled plugin workflow to its exact local file
  theme     Apply an enabled plugin's alias for an existing built-in theme
  catalog   Manage signed curated catalog snapshots and staged versions
  help      Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
  -h, --help            Print help
```

### `ca pricing`

```text
Inspect or refresh the fail-closed official delegation price catalog

Usage: ca pricing [OPTIONS] <COMMAND>

Commands:
  status   Show source freshness and delegation price-card readiness without network
  refresh  Refresh from allowlisted official pages and advisory-check models.dev
  help     Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
      --json
  -h, --help            Print help
```

### `ca runtime`

```text
Manage explicitly selected signed external runtimes

Usage: ca runtime [OPTIONS] <COMMAND>

Commands:
  trust     Pin one exact catalog source and local Ed25519 trust root
  untrust   Remove catalog trust while retaining installed runtime files
  sync      Explicitly fetch and verify one pinned catalog snapshot
  list      List safe catalog and installed-runtime metadata
  preview   Preview exact bytes and destination for install/update or removal
  install   Download, verify, and retain one catalog runtime
  update    Download and activate one newer signed catalog runtime
  rollback  Select one already retained and fully reverified version
  path      Print one fully reverified managed entrypoint path
  remove    Permanently remove exactly one manager-owned runtime tree
  help      Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
  -h, --help            Print help
```

### `ca local`

```text
Manage explicit local GGUF models and supervised llama.cpp runs

Usage: ca local [OPTIONS] <COMMAND>

Commands:
  list      List safe imported model metadata without reading model bytes
  inspect   Reverify and inspect one imported model
  import    Import one user-provided GGUF file
  download  Download one exact GGUF after an explicit metadata preview
  remove    Remove one exact managed model
  serve     Run one managed GGUF through an exact managed llama-server until Ctrl+C
  help      Print this message or the help of the given subcommand(s)

Options:
      --profile <NAME>
  -h, --help            Print help
```

### `ca update`

```text
Explicitly verify, stage, install, and roll back signed ca releases

Usage: ca update <COMMAND>

Commands:
  trust     Pin one exact release source and Ed25519 public key without fetching
  status    Show safe local update state without network or writes
  check     Explicitly fetch and verify the signed release manifest
  channel   Persist the exact release channel used when no version is pinned
  pin       Pin one exact verified version, or clear the pin
  download  Download and retain one exact verified release without executing it
  install   Atomically replace only the invoking executable with an already staged release
  rollback  Atomically restore the retained verified previous executable
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `ca repo`

```text
Inspect local repository state without network access

Usage: ca <COMMAND>

Commands:
  info            Print stable local repository identity and subsystem readiness
  status          Print stable local Git change counts and worktree state
  worktree-setup  Preview the repository-defined worktree setup and exact digest
  help            Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

### `ca shell-integration`

```text
Print CA shell integration

Usage: ca <COMMAND>

Commands:
  zsh   Print completion and terminal-key setup for zsh
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```
