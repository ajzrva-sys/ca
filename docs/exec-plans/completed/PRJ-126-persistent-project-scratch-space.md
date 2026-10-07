# PRJ-126 — Persistent Project Scratch Space

Status: COMPLETE
Owner/session: Codex / current reliability continuation
Last updated: 2026-08-16

## Goal

Add a profile-isolated, canonical-workspace-keyed, bounded and symlink-safe scratch store for human-managed notes, todos, and saved-plan references, plus one read-only bounded model tool.

## Invariants

- Scratch is outside Git, sessions, exports, and automatic prompts.
- Only `/scratch` mutates; `read_scratch` is read-only and cannot propose writes.
- Versioned owner-only atomic storage rejects symlinks, invalid data, more than 256 entries, text over 4 KiB, or serialized state over 256 KiB without changing the existing file.
- Bare mode is visibly unavailable and never persists.
- Saved plans contain only existing session plan IDs and revisions, never duplicated plan bodies or executable commands.

## Milestones

1. Characterize the store, bounds, isolation, atomicity, permissions, and symlink refusal.
2. Add the read-only tool and bounded sanitized rendering.
3. Add the human `/scratch` manager with Cancel-first deletion.
4. Run local gates, reconcile documentation, archive the plan, and complete the requested single delivery commit after combined validation.

## Validation

Focused scratch/store/tool/render coverage passed locally. Combined local default (1448 passed, 1 ignored), all-feature main (1477 passed, 1 ignored), experiment (118 passed, 1 ignored), and CLI (37 passed) passed. Formatting, strict all-feature Clippy, release build, help/version, diff, and snapshot hygiene passed. Wide/narrow empty and Cancel-first deletion snapshots were reviewed. Physical FreeBSD and its native artifact checks were not run for PRJ-126 because the user explicitly paused BSD work; no pass is claimed.

## Recovery

Remove the scratch module, tool registration, process-only manager state, command/input/render surfaces, tests, snapshots, and docs. Existing sessions and provider history require no migration.

## Completion Record

Implemented versioned workspace-keyed owner-only atomic storage, bounded notes/todos/current-plan references, human `/scratch` add/edit/toggle/save/delete operations with default-Cancel deletion, and the read-only filtered 16-KiB `read_scratch` tool. Scratch remains outside Git, sessions, exports, and automatic prompts; bare mode cannot persist.
