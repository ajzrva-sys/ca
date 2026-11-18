# Grok recommendations for CA

This file is a comparison. The three path gaps are in CA 0.1.185. Five later slices are in the tree: an unknown slash stays in the composer, a question paste lands in the open answer, search hits use the search card, plan mode refuses shell writes and read-only children, and a new instruction file is named once. The do-not-port list stays out.

Rechecked `oracles/grok-build` at `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`
(2026-09-29T16:57:09Z). GitHub `refs/heads/main` is that same commit. CA is
0.1.184. The public tree is older than the installed `grok` binary, so
behavior after September 2026 is not in this checkout. No Grok source was
copied into `src/`.

## Recommendations

Landed in CA 0.1.185, in `src/path_lookup.rs`. A missing tool path uses the one parent entry that matches after `U+202F` or `U+00A0` becomes a space, and the result names that code point. A still-missing path names a dropped repo folder or up to three similar siblings, plus the working directory. Tool canonicalize waits 30 seconds, then keeps the path the model sent.

### 1. Resolve a missing filename that differs by a unicode space

Grok `try_resolve_unicode_filename` in
`crates/codegen/xai-grok-tools/src/util/fs.rs` runs when the path is not
found. It lists the parent, once, and keeps a name that matches after
`U+202F` and `U+00A0` become a normal space. One match is used, and the tool
result names the code points. Two matches do nothing. The scan stops at 30
seconds.

CA `read_file` reports `file does not exist` (`src/agent/tools.rs`). A macOS
screenshot name uses `U+202F`, and the model sends a normal space, so the
read misses a file that is in that directory.

On a not-found path, do that one parent listing before the error. Use the
match for the read or edit. Leave the stored path the model sent unchanged
except for the note. Do not fold other characters.

### 2. Name a nearby path when the file is missing

Grok `path_suggestions.rs` appends a hint to `does not exist`. If the model
dropped a repo folder, the hint is `Did you mean …?`. Otherwise it names up
to three siblings in the parent whose names contain the missing leaf. The
cwd is on the same message. The probe is one blocking call with a 100ms cap.
A timeout leaves the bare error.

CA stops at `file does not exist`.

Add that suffix on `read_file`, `search_replace`, `list_dir`, and search.
Keep the cap. Do not walk above the parent.

### 3. Stop a tool canonicalize that does not return

Grok `canonicalize_with_timeout` in the same `fs.rs` waits 30 seconds, then
uses the path it was given. A hung `stat` on a slow volume does not hold the
tool open.

CA calls `std::fs::canonicalize` on tool and workspace paths with no
deadline. The checkout lives under `/Volumes`.

Put the deadline on the tool path resolution that runs before a read, write,
or search. On timeout, continue with the path the model sent. Leave
canonicalize calls that decide trust or install paths as they are.

## Already in CA

Leave these alone.

- Hashline edit, the precise-edit chain including Unicode fold, and the
  confusable hint that names a smart quote without rewriting the file.
- The shell snapshot that replays cwd, exports, functions, aliases, and
  options, and drops secret-shaped names (`src/shell_snapshot.rs`).
- `web_fetch` SSRF refusal and oversized-body spill.
- LSP diagnostics appended after an edit.
- A nearby skill directory named once after a read or edit
  (`src/skill_proximity.rs`).
- A file write lock around the mutation (`src/path_lock.rs`).
- Hosted `x_search` on the xAI request.

## Do not port

- Grok source copied into `src/`.
- Settings, themes, billing, Mixpanel, training consent, the hosted
  marketplace, ACP, feedback, and voice.
- Computer use, video generation, and the deploy stubs.
- The Linux BTRFS and overlay worktree pool. CA already creates worktrees.
- A refusal of every gitignored read. Credential paths and `.env` stay
  refused. Other ignored files stay readable.
