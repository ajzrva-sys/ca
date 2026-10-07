# `ca role-run` Protocol

Status: implemented under `INT-148`; socket-backed final validation pending

`ca role-run` is a narrow, process-scoped interface for a supervising
orchestrator. It is not an alias for ordinary headless mode or for the private
scheduled-worker command. The caller supplies one exact role request on stdin,
and `ca` returns the existing `ca.headless.v2` JSONL event stream on stdout.

## Command

```text
ca role-run --profile NAME
ca role-run --profile NAME --preflight
```

`NAME` must be a nonreserved named profile. `default` and `bare` are rejected.
The profile must set `agent.sandbox_mode = "required"`.

Preflight is offline. It reads the named profile without creating or migrating
configuration, resolving credentials, contacting a provider, or creating
runtime data. It emits one `ca.role_run_preflight.v1` object:

```json
{
  "schema": "ca.role_run_preflight.v1",
  "ready": true,
  "profile": "codex-team",
  "sandbox_required": true,
  "routes": [{"provider": "openai", "models": ["gpt-5.6-sol"]}],
  "issues": []
}
```

A syntactically valid preflight receipt exits successfully even when
`ready` is `false`; callers must inspect `ready` and `issues`. Command-line
parsing failures and failure to emit the receipt remain process errors. This
keeps readiness a typed machine result instead of making it indistinguishable
from a broken invocation.

Normal execution reads one JSON object, bounded to 1 MiB:

```json
{
  "schema": "ca.role_run.v1",
  "request_id": "worker-INT-148-001",
  "provider": "openai",
  "model": "gpt-5.6-sol",
  "reasoning_effort": "high",
  "workspace": "/canonical/worktree",
  "prompt": "Complete the bounded role assignment.",
  "output_schema": "/canonical/worktree/.orchestrator/schemas/worker.schema.json",
  "tools": ["read_file", "apply_patch", "write_file"],
  "grants": [
    {"kind": "tool", "tool": "read_file"},
    {
      "kind": "path",
      "tool": "apply_patch",
      "path_pattern": "/canonical/worktree/src/role_run.rs"
    },
    {
      "kind": "path",
      "tool": "write_file",
      "path_pattern": "/canonical/worktree/src/role_run.rs"
    }
  ]
}
```

Unknown fields are rejected. The workspace must already be canonical and equal
the process current directory. The output schema must be a contained regular
file with no symlink component and must pass the existing structured-output
schema validator.

## Authority

The fixed v1 tool allowlist is:

- Read: `read_file`, `list_dir`, `search_files`, `glob`, `read_todos`, `blame`.
- Write: `apply_patch`, `write_file`.

Every selected read tool requires a tool grant. Write tools require exact
workspace-contained path grants; a tool-wide write grant is invalid.
`apply_patch` and `write_file` are selected together for a Worker role.
Absolute escapes, traversal, private/protected roots, and existing symlink
escapes are rejected before provider traffic.

Tool-wide read authority does not mean host-wide path authority. Each tool call
is revalidated against the exact requested workspace immediately before
execution. On Unix, the validator enumerates each existing directory to require
exact component spelling, then opens that component descriptor-relative with
`O_NOFOLLOW`. The executor repeats descriptor-rooted traversal and rejects
intermediate symlinks, cross-device nodes, nonregular files, multiple links,
root replacement, excessive depth, excessive entries/bytes, and private
`.git`/`.orchestrator` traversal. File reads and blame require a contained
regular single-link file; directory/search/glob walks stay bounded. A genuinely
missing final Worker file is allowed. Role execution never promotes the exact
workspace to a parent Git root.

This boundary is verified on macOS and uses Unix directory-descriptor
primitives. Role-run local tool execution fails closed on non-Unix platforms.
A concurrent case-only rename between directory enumeration and descriptor
open is a narrow residual spelling-check race on case-insensitive filesystems.
The opened descriptor remains authoritative, so this race does not permit an
intermediate symlink traversal or workspace escape.

Role-only mutation never writes through an unchecked path. It reopens the
parent and target without following symlinks, verifies workspace/parent/target
identity and a single regular-file link, computes the complete new contents,
rejects any selected-credential material in that full result, writes a private
same-directory file, revalidates parent and target, then renames atomically.
The dedicated patch engine is forward-only and cancellation-aware. It caps the
source at 262,144 lines, the patch at 65,536 lines and 2,048 hunks, applies the
existing output-file byte limit, and validates both old- and new-file
coordinates including zero-count ranges. Symlink, hardlink, nonregular-file,
overflow, malformed patch, and detected reopen races fail closed.

Persistent Deny rules remain ceilings. Persistent Allow and Ask rules provide
no positive authority. The validated request grants are the only positive
authority, and approval mode is always `RequireApproval`. A call outside those
grants emits `approval_required`, makes no change, and exits with code 4.
Malformed retained Deny rules make the named profile unavailable rather than
silently weakening the ceiling.

Role runs do not load plugins, skills, memory, MCP or LSP servers, hooks,
subagents, workflows, jobs, email, browser tools, or model-visible network
tools. They do not persist sessions or change configuration. Provider traffic
is the only network use. Normal execution resolves only the credential for the
exact selected provider/model route; preflight resolves no credential.

## Output and completion

Stdout contains only newline-delimited `ca.headless.v2` envelopes. Diagnostics
are bounded and sanitized. The selected resolved credential is held only as an
opaque redaction value and is removed from envelopes, structured output, usage
metadata, diagnostics, and tool-continuation material. Before any role write,
the same matcher scans the complete resulting file, so a credential composed
across multiple edits is rejected before publication.

Success requires exactly one object-valued `structured_output` event followed
by one final `completion`. Every envelope uses one nonempty run ID and a
zero-based monotonic sequence. A model response that does not call
`report_structured_output`, calls an unavailable tool, or emits a malformed
payload fails closed; provider-controlled call IDs, tool arguments/results,
and protocol diagnostics are not copied verbatim into the role stream.

The existing headless exit classes remain in force: input errors use 2, setup
errors 3, approval-required 4, provider transport/protocol errors 5/6,
turn-limit errors 7, internal errors 8, and interruption 130.

## Operational guidance

Run preflight first. Launch one role process in the canonical worktree, write
the request, close stdin, and consume stdout and stderr concurrently. Apply an
outer timeout and terminate the whole process group on cancellation. Treat any
nonzero exit, failure event, missing structured output, or completion ordering
error as a failed role. A supervising orchestrator must still run its own
post-execution workspace guard and verification before accepting changes.

Latest-source focused validation passes 34/34 role tests, both patch-coordinate
tests, strict all-target/all-feature Clippy, formatting, an offline all-feature
release build, and help/version smoke checks. The final release SHA-256 is
`f159fa419d8ac3061a6d905617996c58a27a268ca7254df8b3d666d39d184222`.

The real-binary suite uses loopback fake providers. This managed sandbox denies
listener creation, so its current result is 3 provider-free passes and 14
failures at `TcpListener::bind` with `Operation not permitted`, all before
provider traffic. Earlier pre-final binaries passed real-orchestrator role and
actual-`ca` MCP compatibility, but a final-binary rerun remains required in a
host that permits loopback and the orchestrator isolation backend. These tests
use disposable profiles/workspaces and are not live-provider evidence. INT-148
therefore remains active rather than accepted for production.
