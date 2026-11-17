# Silo'd Agent Orchestration in ca — Full Discussion

> Exported conversation. Two questions, two responses, plus a reasoning summary.

---

## Question 1 (user)

Consider ca silo'd agent orchestration of the following, and how it would be implemented. Focus only on the orchestration, the goal being each agent not knowing what the other is doing- the following is just an example:

> ◆ Thought for 7.3s
> I'll map the actual project structure to build an accurate task graph. Let me inspect the relevant directories and files.
>
>   ✓ Explored
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ru…
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results
>
> ◆ Thought for 0.9s
>   ✓ Explored
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     list_dir /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ru…
>
> ◆ Thought for 3.3s
>   ✓ Explored
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>
> ◆ Thought for 1.1s
>   ✓ Explored
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     read_file /Volumes/projects/esxi/esxi/bhyve-engineering/results/parity-vmxnet3-ha…
>     read_file /Volumes/

---

## Response 1

Good framing — "silo'd" is the key constraint, and it changes the architecture. Here's how I'd think about the orchestration layer in ca, grounding on what ca already has (`src/agent/` subagents, `crates/ca-automation-protocol` contracts, and the parent's admission/scheduling role).

### The model: one coordinator, isolated lanes, contract-only communication

Everything funnels through a single **coordinator** (ca's main loop). Workers are spawned as subagents in **lanes** that cannot see each other. The only information that moves is:

- **Downward** — a narrow, need-to-know task brief.
- **Upward** — declared output artifacts/evidence, never raw transcripts or sibling state.

```
             ┌─────────────────────────────────┐
             │  coordinator (owns task graph)   │
             │  admission, scheduling, merge    │
             └──────┬───────────────┬───────────┘
        brief+grant  │   handoff     │  brief+grant
        ┌────────────▼──┐   (declared │ ┌────────────▼──┐
        │  lane A        │  artifact) │ │  lane B        │
        │  own worktree  │◄────────────┤ │  own worktree  │
        │  own tool set  │             │ │  own tool set  │
        └───────────────┘             │ └───────────────┘
                        lane B never sees lane A's workspace or transcript
```

The isolation is what makes them "silo'd." No shared mutable workspace, no shared conversation buffer, no sibling transcript. Each lane holds only: its brief, its granted paths/tools, and any handoff artifacts the coordinator explicitly passes in.

### What the transcript example is actually showing

That `list_dir` / `read_file` sequence is the **discovery/planning phase** — a single read-only agent mapping the real project structure before committing to a task graph. It's not the workers; it's the architect. In orchestration terms:

1. **Discover** — one bounded, read-only lane explores and emits a task DAG (tasks + per-task grants + dependency edges).
2. **Admit/schedule** — the coordinator turns that DAG into isolated lanes. This is exactly what `crates/ca-automation-protocol`/`ca-automation` exist for: ca owns admission and scheduling against a defined per-run contract.
3. **Fan out** — each task runs in its own lane with a disjoint grant.
4. **Handoff** — only declared artifacts cross lane boundaries (`accept_cross_lane_handoff`), and a lane doesn't receive the producing lane's context.
5. **Merge/review** — the coordinator (or a separate reviewer lane) integrates, with a review loop if the merge is contested.

### How ca's existing primitives express the silos

This maps cleanly onto the graph configuration available here:

- **Isolation = disjoint path grant or separate worktree.** Each lane gets `worktree_id` or non-overlapping `paths`. Two lanes pointing at the same files is precisely what you forbid for true siloing.
- **Narrow capability grant.** Per-lane `tools`, `commands`, and `network` flag. A lane that only needs to edit one directory shouldn't be able to list the repo root, let alone read a sibling's output.
- **Contract-only handoffs.** `accept_cross_lane_handoff` means a lane only takes inputs the graph declares. No ambient access to "what the other agent did."
- **Evidence over claims.** Completion is host-verified (`artifact` with hash/bytes, `closed_command`, `tool_success`, `human_review`). The coordinator merges based on artifacts, not on "I think I finished."
- **Bounded attempts and budget.** `max_attempts` per lane and one finite graph budget (USD cap or unpriced-route decision with a physical-request cap) — so a stalled lane can't silently loop.
- **The workflow layer above it** (`plan_workflow`) gives the author → review → revise → implement sequence when you want a human-or-explicit review gate between phases rather than a pure fan-out.

### The isolation mechanics in practice

To actually get "each agent doesn't know what the other is doing," four things have to be true at runtime, not just in the config:

1. **No shared context object.** Each subagent gets a fresh conversation seeded only with its brief. The `app`/session state the parent holds is not inherited by lanes.
2. **Disjoint writes.** Separate worktrees (preferred) or strictly disjoint path grants; otherwise two silos collide at merge and you've lost determinism.
3. **Explicit, versioned handoffs.** A handoff is a named artifact with a contract (path, schema, hash), not "read whatever lane A produced." This is what the `-protocol` crate formalizes.
4. **Merge is a distinct role.** The coordinator merges on the declared contract; conflicts fail the merge and route to review rather than being silently resolved inside a lane.

### Why silo'd beats a shared-brain fan-out

- **No cross-contamination** — an agent can't "cheat" by reading a sibling's half-finished state.
- **Reproducible runs** — identical brief + grant + input artifacts = identical lane behavior, which the `SOURCE-MANIFEST`/SHA-pinning pattern in your bhyve work already relies on.
- **Conflicts surface at the merge point**, not mid-flight where they're harder to attribute.
- **Independent retry** — a failed lane re-runs alone; the coordinator doesn't have to replay the whole graph.

### Real trade-offs / where it gets hard

- **The coordinator is the bottleneck and single point of failure.** All knowledge lives there; if it mis-summarizes, every handoff degrades.
- **Briefs must be precise or lanes stall.** Need-to-know isolation amplifies any under-specification, because the worker can't reach for context.
- **Over-isolation causes duplicate work** (two lanes re-deriving the same fact). The fix is making shared facts *declared inputs* fed in by the coordinator, not letting lanes read each other.
- **Idempotent merge is non-trivial.** If lanes produce overlapping outputs, the contract must define a deterministic resolve (reject, last-write, or route-to-review).

One caveat on provenance: the harness documents ca's subagent, automation-protocol, and admission/scheduling surface, but I haven't read the actual `src/agent/` and `ca-automation-protocol` source to confirm exactly which of these mechanisms are already wired versus planned. What I described as "existing primitives" is the graph/workflow tooling I can see; the degree to which `ca-automation-protocol` already enforces disjoint-workspace isolation and contract-only handoffs is something I'd verify in the source before claiming it ships.

If you want, I can formalize this into an ExecPlan or a cross-provider `plan_workflow` (architect → isolated implementers → merge/review) with concrete lane grants and handoff contracts — say the word and I'll draft it for approval rather than keep it at the design level.

---

## Question 2 (user)

review the tradeoffs and potential solutions-

> ## Real trade-offs / where it gets hard
>
> - The coordinator is the bottleneck and single point of failure. All knowledge lives there; if it mis-summarizes, every handoff degrades.
> - Briefs must be precise or lanes stall. Need-to-know isolation amplifies any under-specification, because the worker can't reach for context.
> - Over-isolation causes duplicate work (two lanes re-deriving the same fact). The fix is making shared facts declared inputs fed in by the coordinator, not letting lanes read each other.
> - Idempotent merge is non-trivial. If lanes produce overlapping outputs, the contract must define a deterministic resolve (reject, last-write, or route-to-review).

---

## Response 2

Here's a sharper review. The four items aren't four independent problems — they're one problem from four angles: **information hiding removes implicit coordination, so every coordination need must be externalized (contracts, declared inputs, DAG edges, conflict policy) up front.** Each "tradeoff" is a symptom of a coordination need that wasn't made explicit. Reviewing them through that lens:

### 1. Coordinator as bottleneck / SPOF

**This conflates two distinct failure modes, and they have different fixes.**

- **Throughput bottleneck** — the coordinator serializes spawn/collect/merge. Mitigation: keep it a *thin router*, not a reasoner. It moves declared artifacts and checks contracts; it must **never summarize**. Summarization is the lossy step that turns a router into a single point of corruption. A handoff should be "raw artifact + contract," never "coordinator-authored prose about the artifact."
- **Correctness SPOF** — a wrong merge decision degrades everything downstream. Mitigation: **fail-stop + replayable ledger.** The coordinator persists the graph and verified handoffs; it's restartable and resumable from the last verified handoff, not from memory. If it can't verify a handoff, it fails the lane rather than guessing. A bad decision becomes a visible, re-reviewable artifact instead of silent corruption.
- The residual irreducible SPOF is scheduling/merge *judgment*. That can be bounded by a **separate reviewer lane** that reviews only the coordinator's own merge output — same silo rules, but pointed at the coordinator.

Net: "all knowledge lives there" is the actual bug. The coordinator should hold the **coordination view** (graph, grants, order), not the **knowledge** (what the work means). Those are separable.

### 2. Precise briefs or lanes stall

**"Write more precise briefs" is the wrong lever — precision is unbounded and can't be guaranteed a priori.** The right levers:

- **Isolate state, not authority communication.** A lane can't see siblings, but it should have a **typed question channel back to its one parent** ("need clarification," bounded questions). Isolation of workspace doesn't require isolation from the only entity authorized to disambiguate. This alone removes most stalls.
- **Declared inputs, not discovered context.** The coordinator computes what's need-to-know and passes it down as immutable declared facts. Under-specification becomes a coordinator error caught by the lane's "missing input" signal, which loops back instead of hanging.
- **Allow bounded probing *within the grant*.** The silo is about not reading siblings, not about not reading your own inputs. Over-restricting a lane's read access to its own granted paths manufactures stalls for no isolation benefit.
- **Make "done" machine-checkable** (artifact/sha256/closed-command), so a stall is *detectable* and bounded by `max_attempts`, not silent.

Under-specification doesn't need perfect briefs; it needs to be a **visible, addressable event** with a loop-back path.

### 3. Over-isolation causes duplicate work

**The "declare shared facts" fix is right, but only after classifying by cost and mutability — dedupe is itself coordination overhead, and sometimes recompute is cheaper than the handoff.**

- **Cheap + stable fact** → let lanes recompute. The coordination cost of sharing exceeds the compute cost.
- **Expensive + stable fact** → a dedicated **"facts" lane runs first** and publishes once; the coordinator feeds the same artifact to all consumers. Lanes never learn of each other.
- **Expensive + mutable** → that's a *dependency*, not a fact. It's a DAG edge: the consumer runs after the producer and receives the artifact as a declared handoff.
- **Dedupe at the coordinator, not between lanes.** An artifact cache keyed by hash lets the coordinator reuse a computed input without any lane ever coordinating. Isolation is preserved; the dedupe is invisible to workers.

Don't over-engineer: accept some recompute as the *price of independence*. Only dedupe where the cost differential is measurable.

### 4. Idempotent merge is non-trivial

**Correct, and it's the genuinely hard one. The strategy is: prevent overlap by construction so merge is mechanical, and reserve resolve-policy for the residual.**

- **Partition the output space in the grant.** Disjoint files, or disjoint regions with a deterministic merge (section-scoped manifest ownership, sorted-map merge). A lane physically *cannot* write outside its region. This eliminates most conflicts before merge.
- **Declare the conflict policy per merge unit at DAG time**, not discovered at merge: reject-and-route (safe default), last-write under a deterministic total order (lane id or declared precedence), or three-way merge against a known base. The policy must be in the contract up front.
- **Merge is its own isolated lane.** It receives *only* declared outputs + base + policy, applies deterministic merge, emits the merged artifact. It never sees transcripts — so it's auditable and retryable.
- **Choose merge by artifact kind.** JSON/manifest → merge-by-key with conflict detection; source → three-way text merge against the common base commit; append-only logs → concatenation under stable sort.
- **The real semantic safety net is a post-merge verification gate.** Merge policy resolves structural overlap; the test/build gate catches semantic conflict. Never silently resolve semantic conflict — reject is the default, and last-write is only safe for provably independent regions.

### The one principle underneath all four

Separate **coordination** from **knowledge**. Lanes are silo'd on knowledge (each knows only its brief + declared inputs + granted paths), while the coordinator holds the full coordination view and nothing else. Most pain in silo'd orchestration comes from conflating the two — either leaking knowledge into lanes (breaking the silo) or forcing lanes to coordinate (creating stalls, duplicates, and merge ambiguity). Externalize coordination into contracts once, up front, so lanes never need it at runtime.

If you want this turned into an executable design, I can draft an ExecPlan (thin-routing coordinator, question channel, facts lane, merge lane with declared conflict policy) or set up a cross-provider `plan_workflow` demonstrating the lanes — just say so and I'll produce it for approval rather than keep it at the whiteboard level.

---

## Reasoning summary (in place of raw internal scratchpad)

> Note: verbatim internal chain-of-thought isn't reproduced here; this is a concise account of the actual considerations behind each response.

**Response 1:**
- Anchored the design in ca's real surface (subagents, `ca-automation-protocol`, admission/scheduling) rather than inventing a generic framework.
- Treated "silo'd" as the load-bearing requirement: isolation is defined negatively (no shared workspace, no shared transcript, no sibling access), which forced everything else to be contract-mediated.
- Read the transcript sample as the discovery/planning phase, not the workers — because its only tool calls are read-only `list_dir`/`read_file` mapping, which is architect behavior.
- Kept an honest provenance caveat: the graph/workflow primitives were observable, but whether `ca-automation-protocol` already enforces disjoint-workspace + contract-only handoffs needed source verification, so I didn't overclaim.

**Response 2:**
- Collapsed the four "tradeoffs" into one root cause: information hiding removes implicit coordination, so coordination must be externalized up front. This reframed the list from separate problems into facets of one design requirement.
- Distinguished throughput-bottleneck from correctness-SPOF because they demand different fixes (thin routing vs. fail-stop ledger), and separated coordination-view from knowledge as the real fix for both.
- Rejected "write more precise briefs" as the lever for stalls — precision is unbounded and can't be guaranteed a priori — in favor of a typed loop-back question channel plus machine-checkable completion.
- Applied cost/mutability classification to dedupe so the answer wasn't "always share facts" but "dedupe only when the cost differential is measurable."
- Made merge deterministic by construction (partition output space, declare conflict policy at DAG time, merge as its own isolated lane) instead of hoping a resolve-policy cleans up after overlap.
