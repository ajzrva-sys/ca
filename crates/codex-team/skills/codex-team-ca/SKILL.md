---
name: codex-team-ca
description: Supervise one safe codex-team cycle through its pinned MCP server.
---

# `codex-team` supervisor

Use this skill only with a server pinned to the intended clean target worktree.
`codex-team` remains the scheduler, verifier, guard, and committer.

1. Call `status` and `doctor` before proposing any mutation or delegation. Stop if doctor is
   unhealthy, the repository is dirty, a job is active, a human pause exists,
   or a pending commit already exists.
2. Prefer `repo_query` for mechanical exploration. Batch at most eight tracked-file
   reads, text searches, file listings, Git status/diff summaries, or bounded
   history requests against the exact current revision. Inspect the receipt,
   including readiness, revision, truncation, and per-action status, before using
   its bounded context. Refresh status after a stale receipt. Never request
   untracked, symlinked, hard-linked, private, or secret-shaped paths.
3. Prefer `delegation_start` for a typed evidence chain or one contained local
   observation. Poll `delegation_status`; do not retry the start. Verification
   may run only when the readiness receipt says production isolation is
   available. Copy marker and log paths must stay inside the pinned repository.
   Cancelling observation stops only the watcher and never signals the observed
   PID.
4. Treat the delegation policy literally. `prefer` may fall back only after an
   explicit declined or unavailable receipt; `require` never falls back. A real
   verification failure is evidence, not permission to ask a model to retry the
   same work silently. Record only categorical `delegation_feedback`, and use a
   human correction only after exact human confirmation.
5. Learned `DELEGATION.json` files are inert typed proposals. Never execute a
   learned script, command string, binary, or executable package member. Use an
   approved manifest only for one unambiguous structural match at its exact
   approved repository revision, and inspect every receipt before continuing.
6. Preview a complete typed task before applying it. Show the human the task,
   validation result, import digest, and repository revision.
7. Apply only the exact preview digest and revision after human approval.
8. Start only one step with the current revision. If start returns a job ID,
   poll `step_status`; never retry `step_start` for the same work.
9. Never edit the target repository while a job is running.
10. Treat cancelled, timed-out, or interrupted jobs as possibly partial. Refresh
   status and ask the human what to do.
11. Show the pending commit's exact paths, digest, message, guard evidence, and
   verification evidence. Ask the human before calling `commit_pending`.
12. Ask the human before every disposition call and use the exact current pause
   fingerprint and revision.
13. Never auto-continue, start concurrent steps, broaden task scope, invoke Git
   directly, discard patches, install binaries, merge, deploy, or modify a real
   runtime profile.
