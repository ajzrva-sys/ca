# Autonomous Learning and Delegation Cockpit

`ca` exposes the `codex-team` learning supervisor as a host-rendered Plan-style
cockpit. Open it with `/self-improve [server]` or Settings → Autonomous learning.
It shows enrolled evidence profiles, bridge health, queued Dream/Distill work,
the active objective and workflow stage, candidate class/author/judges, replay
progress, failures/cooldowns, shared core and provider overlays, learned skill
and recipe revisions, recorded/avoided usage, vault space, component A/B slots,
and recent candidate/evaluation/deployment events.

Pause, Resume, Rollback ca, and Rollback codex-team are direct typed MCP calls.
They are never placed in a prompt and never cause a provider request. Resume and
rollback bind to the exact supervisor revision shown in the cockpit. Rollback
also pauses learning for review.

Delegation remains `same` by default. Settings → Delegated agent selects a fixed
configured provider/model, including a different provider. Settings → Delegation
policy toggles learned routing. Enabling learned routing writes an explicit
allowlist of the configured provider/model routes; a learned selection outside
that list fails instead of falling back. Route selection never changes after a
request begins.

This delegation setting currently governs ordinary `ca` child agents globally.
The orchestrator's CEO, Project Manager, Senior Dev, and Worker role policies
still require a dedicated per-role editor.

The cockpit does not enroll the machine, edit provider credentials, install a
binary, or grant source-write authority. Enrollment and the safety kernel remain
owned by the stable Rust supervisor. This artifact-learning slice never invokes
source transitions, builds, commits, or candidate binary activation.

## Enrolled profile bridge

`codex-team self-improve enroll` binds every repeatable `--ca-profile NAME` and
one `--ca-execution-profile NAME` into the exact human confirmation digest.
The evidence profiles are the only `ca` profiles whose sessions can enter this
loop. Enrollment-v1 records have an empty evidence allowlist.

For the enrolled default profile, the stable supervisor harvests due native and
Codex active/archive evidence into immutable Dream/Distill intents. The UI
reports queued work before provider processing. The active orchestrator binary
uses the offline bounded bridge:

```text
ca learning-bridge preflight --profile NAME
ca learning-bridge export --profile NAME --enrollment-digest SHA256
ca learning-bridge complete --profile NAME \
  --enrollment-digest SHA256 < completion-request.json
```

Export performs no provider request and never acknowledges evidence. Completion
binds the original intent, frozen evaluation receipt, terminal outcome, and
exact memory or inert SKILL.md artifact. It is idempotent; only successful
journaled publication or terminal rejection acknowledges evidence. The old
acknowledgement command remains a compatibility surface. Corrupt/stale markers
fail closed, and unenrolled profiles retain direct Dream/Distill behavior.

The stable supervisor launches one bounded active-binary worker stage per tick.
Foreground model leases preempt background provider work. The worker freezes
evaluation cases before candidate generation, uses the restricted read-only
`ca role-run` profile, and promotes only correctness-first replay winners.
Credentials, environment values, canonical workspace paths, unknown tool
arguments, and unsanitized session material are excluded from bridge records.

`ca learn` owns status, one-shot run, pause/resume, and the exact-confirmed
single-service lifecycle. Knowledge-only enrollment grants automatic
publication authority only for replay-approved Dream memory and inert skills;
source/binary/routing/permission/provider-setting/role-prompt/executable/plugin/
dependency changes remain forbidden. Both repositories' matrices and the
process-level fake Codex/DeepSeek/MiMo activation pilot pass. The release login
service remains disabled until LRN-166's exact activation confirmation; source
transitions and deployments are outside this learning authority.
