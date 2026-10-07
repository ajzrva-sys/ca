# macOS Inter-Provider Delegation

`ca` can route an explicitly complexity-labelled `task` or `spawn_agent` child
to the cheapest sufficiently capable configured OpenAI, Xiaomi MiMo, or
DeepSeek route. Omitting `complexity` inherits the active provider/model; models
are instructed to omit it unless the user explicitly requests inter-provider
cost routing. Explicit routing is fail-closed: no provider request starts unless
the route has a usable runtime credential, a supported billing mode, a
sufficient capability ceiling, and an official price card verified within the
last 30 days.

The initial support and acceptance target is macOS arm64.

## Configure Xiaomi MiMo

Open `/provider`, choose Xiaomi MiMo, then choose one setup method:

- **Use saved Token Plan credential** appears when a connected MiMo route
  already uses an exact CN, Singapore, or Europe Token Plan endpoint. It reuses
  the saved Keychain-backed credential and collects only the missing plan
  metadata; it does not reopen the browser or ask for the key.
- **Browser authorization** accepts either Xiaomi PAYG or Token Plan
  authorization. CA classifies the returned credential together with Xiaomi's
  returned endpoint. PAYG is metered and delegation-eligible only on Xiaomi's
  exact official PAYG endpoint; Token Plan authorization continues to the
  tier, term, and USD allocation screens before anything is persisted.
- **Manual Token Plan key** accepts a masked key beginning with `tp-`. Select
  the same Token Plan region shown in Xiaomi: CN, Singapore, or Europe. `ca`
  uses the corresponding documented endpoint and never writes the key into
  `config.toml`; only the profile-scoped credential reference is persisted.

Token Plan setup also asks for Lite, Standard, Pro, or Max; monthly or annual;
and the actual USD amount paid. The official list amount is shown. If the two
amounts differ, the safer default is Cancel and an explicit confirmation is
required before the route can become eligible. Xiaomi publishes no supported
account-plan API, so `ca` does not scrape private account endpoints.

CA fails closed when a `tp-` credential is paired with a non-Token-Plan
endpoint or a non-`tp-` credential is paired with a Token Plan endpoint. These
mismatches are rejected before probing, persistence, or usage recording.

The browser callback listener tolerates speculative Firefox loopback
connections. Leave `ca` running until the browser reports completion. The
listener expires after five minutes.

## Configure DeepSeek

Use `/provider` to connect DeepSeek with a masked API key. Automatic delegation
accepts only the exact official `https://api.deepseek.com` or
`https://api.deepseek.com/v1` endpoint. A proxy or custom OpenAI-compatible
endpoint can remain available for ordinary use, but is delegation-ineligible
because CA cannot establish its billing identity from DeepSeek's official
price card.

## Official pricing

Inspect the profile's cached official cards without network access:

```text
ca pricing status
ca pricing status --json --profile work
```

Refresh from allowlisted first-party HTTPS pages:

```text
ca pricing refresh
ca pricing refresh --json --profile work
```

Refresh publishes the catalog atomically only when every OpenAI, Xiaomi, and DeepSeek
card parses and validates. A failed or partial refresh leaves the previous
catalog intact. Cards older than 30 days cannot route. `models.dev` is fetched
only as an advisory MiMo PAYG cross-check and never establishes Token Plan
eligibility.

The authoritative sources are the official [Sol model
page](https://developers.openai.com/api/docs/models/gpt-5.6-sol), [Terra model
page](https://developers.openai.com/api/docs/models/gpt-5.6-terra), [Luna model
page](https://developers.openai.com/api/docs/models/gpt-5.6-luna), [Xiaomi
PAYG pricing](https://mimo.mi.com/docs/en-US/price/pay-as-you-go), and [Xiaomi
Token Plan documentation](https://mimo.mi.com/docs/en-US/tokenplan/subscription).
DeepSeek V4 rates come from its official [API pricing
page](https://api-docs.deepseek.com/quick_start/pricing/).

`/usage` reports metered direct OpenAI and MiMo PAYG dollars as incremental
cost. MiMo Token Plan reports exact credits and rounded dollars as
**subscription allocation**, not as an incremental charge. Missing or inconsistent provider token subsets, stale
cards, and requests spanning the MiMo off-peak boundary display unknown cost.
MiMo Token Plan credits receive **20% off during 16:00–24:00 UTC**, which is
9:00 AM–5:00 PM PDT or 8:00 AM–4:00 PM PST. The discount is applied only when
the complete request lies unambiguously inside the UTC interval. Delegation
forecasts intentionally use peak credits and state that the discount is not
included; discounted actual usage is labeled `20% off-peak`.

DeepSeek usage is also metered incremental USD. CA requires both official
`prompt_cache_hit_tokens` and `prompt_cache_miss_tokens`, and verifies that
they sum to total prompt tokens before calculating actual cost. Missing or
inconsistent cache subsets keep cost unknown rather than charging all input at
one guessed rate. DeepSeek reasoning and visible output share the same output
rate, so no reasoning split is required for an exact total-output charge.

Offline `ca --doctor` output includes price freshness, billing readiness,
capability, and route eligibility for each configured provider.

## Route a child

For normal delegation, open `/config`, select **Delegated agent**, then choose
**Same as current agent** or any exact configured provider/model. The choice is
profile-scoped and applies to `task` and `spawn_agent` when they omit
`complexity`. It does not change the foreground model, child grants, approvals,
workspace scope, network policy, or provider credentials.

Both child tools accept an optional `complexity`:

```json
{
  "subagent_name": "plan",
  "prompt": "Find the parser entry point and summarize it",
  "complexity": "simple"
}
```

```json
{
  "task": "Inspect the fixture and report the failing invariant",
  "tools": ["read_file", "search_files", "list_dir"],
  "complexity": "standard"
}
```

Accepted values are `simple`, `standard`, and `complex`. Omitting complexity
preserves the current parent route, including a ChatGPT-backed Codex route.
Supplying it is a deliberate paid-route request and invokes automatic
selection: estimated task/context input plus 4,096 output tokens, uncached
metered input, and peak MiMo credit consumption. DeepSeek forecasts therefore
use the cache-miss rate. An unavailable explicit route fails once and never
falls back to the parent. Ties prefer the lower capability ceiling and then
lexical provider/model identity.

Current ceilings are:

| Route | Ceiling |
|---|---|
| `openai/gpt-5.6-luna` | simple |
| `deepseek/deepseek-v4-flash` | simple |
| `mimo/mimo-v2.5` | simple |
| `deepseek/deepseek-v4-pro` | standard |
| `mimo/mimo-v2.5-pro` | standard |
| `openai/gpt-5.6-terra` | standard |
| `openai/gpt-5.6-sol` | complex |

## First-party protocol contracts

Built-in capability and wire behavior applies only to exact official endpoint,
protocol, and model combinations. A proxy named `openai`, `mimo`, or
`deepseek` stays a generic OpenAI-compatible route. User `model_profiles` and
legacy `model_context` entries remain higher precedence than built-in values.

New MiMo definitions use Responses. Existing saved Chat Completions definitions
remain valid and retain `thinking.type` plus exact `reasoning_content`
continuation. DeepSeek remains Chat-first and may be configured explicitly for
its Responses alternative. OpenAI/Codex uses Responses, exact supported
reasoning effort, `store:false`, and complete output-item replay.

For direct `api.openai.com` routes, the provider editor exposes prompt caching
as **Automatic** (the default), **Stable prefix**, or **Off**. Stable prefix
marks the last leading instruction block and uses a hashed session key. Cache
keys are runtime-only and never enter requests views, traces, sessions, or
diagnostics. Cache fields are never sent to Codex OAuth, MiMo, DeepSeek, or a
custom proxy.

Live contract verification is explicit and may incur provider usage:

```text
ca --doctor --connect --provider-contract openai
ca --doctor --connect --provider-contract mimo
ca --doctor --connect --provider-contract deepseek
```

The option is repeatable. Each concrete configured route performs at most two
no-retry requests: one synthetic local tool call and one continuation. It has
no filesystem, network, shell, MCP, or remote-tool authority and writes no
session or usage ledger. The versioned report contains only provider/model,
check booleans, a bounded status, and request count—not credentials, prompts,
reasoning, or tool arguments. Ordinary `ca --doctor` output is unchanged.

The tool result includes the selected provider/model and forecast. Selection
finishes before provider traffic and usage recording. Once a child starts,
`ca` never retries it on another provider.

## Conversational approval and runtime authentication

In AutoReview, asynchronous `spawn_agent` calls run without a prompt only when
their tool list is nonempty and every grant is a registered subagent tool that
is read-only and has no network, external-communication, or nested-agent
capability. `commands` must be absent or empty. Unknown, empty, write, shell,
web, communication, nested-agent, or command grants still prompt. Persistent
Deny and Ask rules retain precedence, strict Allowlist still prompts, and the
synchronous `task` tool is unchanged.

Route tables retain provider specifications rather than credential-bearing
clients. At child start, non-Codex specifications construct a transient client
directly. `openai-codex` specifications first use the canonical runtime
provider resolver, which loads or refreshes the cached ChatGPT OAuth session
before constructing the client; concurrent child resolution is serialized.
Authentication failure happens before provider traffic and usage recording,
and runtime tokens remain runner-only. This matches the official Codex
[authentication contract](https://learn.chatgpt.com/docs/auth), including
automatic token refresh during active ChatGPT-authenticated use.

If a provider returns the same terminal error for adjacent completed
`spawn_agent` calls in one batch, the TUI shows one collapsed card with the
count and one error body. Expanding it lists every original call in order. This
is presentation-only: call IDs, results, status, session data, hooks, and
provider continuation messages remain individual.

## Headless restrictions

Headless main runs advertise `task` and `spawn_agent` unless tool selection
explicitly excludes them or an exact persistent rule hard-denies them.
Headless children are always read-only, even with `--yolo`, `--auto-review`, or
`--allow-network`:

- allowed: bounded file read, list, search, glob, lint/todo/blame metadata, and
  pinned skill reads;
- rejected before spawn: shell command grants, file writes, network tools,
  interactive tools, and mutating or unbounded named agents.

Normal, error, cancellation, and contract-failure exits shut down the child
manager. If a main response finishes while asynchronous children remain
active, `ca` cancels and reaps them and returns an `incomplete_subagents`
failure instead of silently succeeding.

Only the explicit child task and bounded context summary cross providers.
Credentials and opaque reasoning state never do.
