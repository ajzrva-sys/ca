# FLT-311 — Real-provider multi-file website qualification

Status: COMPLETE — installed ca 0.1.69 passes (2026-10-28).

## Outcome

Normal interactive `ca --filter` used the installed signed executable and real
native `openai-codex/gpt-6.1-sol` route on a disposable six-file website. ca alone
resolved existing authentication; the driver did not inspect credentials. The
site had four private directories, HTML, linked CSS, an entry/shared JavaScript
pair, SVG and PNG assets, private page/script data, relative/root-relative links,
and a stylesheet query string/fragment. Native file tools only were enabled in
the disposable configuration; automatic review/delegation were disabled there.

The real model followed source links/imports, read the SVG, and performed three
minimal native edits: HTML class `card` → `card ready`, CSS color `#884422` →
`#2255aa`, and JavaScript `label(value)` → `label(value).toUpperCase()` at the
requested assignment. All three files match exact expected bytes. Shared JS,
SVG and PNG hashes are unchanged. All six link/import targets and their original
spellings survive, including query/fragment. No aliases remain in local source;
no files were added or removed.

## Evidence

- Actual run: `scratch/FLT-311/provider-installed-1/receipt.json`, 86.03 seconds.
  All 14 unique HTTPS request/attempt pairs returned HTTP 200 and completed
  streams. All 13 native calls succeed: eight reads, two listings, three edits.
  Final marker, Done state, orderly exit 0 and terminal restoration pass.
- Outgoing final plaintext request fields contain no checked private directory,
  file, path or data values. Independent audit tests 39 originals and 471 unique
  raw/encoded spellings, including relative/absolute paths. The parent's expanded
  audit separately checks 407 spellings. Both report zero leaks.
- One 1,090,525-byte trace has 1,295 contiguous events. Read results are complete;
  model-issued arguments match their continuation history exactly. Source links
  resolve through the native host context. No plaintext-bound omissions occur.
- Exact file, link and asset checks: `provider-installed-1/restoration-audit.json`.
  Independent audit and evidence hashes: `scratch/FLT-311/independent-audit.json`.
- All 111 production configuration/session metadata entries match before/after
  and independent current readback. Fixture configuration and installed binary
  SHA-256 `c7d7ed5efc789312e9c7b9e776d07960e4f0435bb34799fd555a21a9ee3d7a53`
  are unchanged. No runtime settings, production sessions or running user
  processes were changed.

## Capture limits and completion

The captures are ca's final application requests immediately before HTTPS
execution, not remote packet captures. Requests 6–14 each intentionally redact
one opaque encrypted reasoning replay field. Plaintext request fields have no
detected omissions; do not describe these as byte-complete encrypted bodies.
The PNG was preserved by checksum rather than read by the model. No browser or
visual-layout qualification is claimed.

No Rust behavior changed, no release was rebuilt, and no installation was
repeated. The passing FLT-310 full matrices remain the source baseline. A prior
documentation updater had stopped at a historical heading after saving valid
installation receipts; stale FLT-310 summary banners were reconciled with those
receipts during this task. No provider or workflow repair was necessary.

Final evidence index: `scratch/FLT-311/final-assessment.json`.
