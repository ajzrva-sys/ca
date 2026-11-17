# FLT-313 — Real-provider default-startup website qualification

Status: COMPLETE — installed ca 0.1.70 passes without `--filter` (2026-10-28).

## Outcome

Normal interactive entry used `/Users/aji/.local/bin/ca` and the real native
`openai-codex/gpt-6.1-sol` route on a disposable six-file website. The launch has
no `--filter` argument. Saved `enabled=true`, `structural=true` and
`hide.filenames=true` activate the mode; the pre-input screen shows “Filter
active” and “Selected values only” despite a saved legacy Latin control. The
actual user's configuration digest matches FLT-312's verified activation; the
disposable profile mirrors those privacy switches without copying credentials
or unrelated production settings. ca alone resolves native authentication.

The site includes four private directories, HTML, linked CSS, entry/shared
JavaScript, SVG and PNG, private page/script data, relative/root-relative links,
and a stylesheet query string and fragment. The model followed source links,
read the shared module and SVG, and completed exactly three native edits:
HTML class `card` → `card ready`; CSS color `#884422` → `#2255aa`; JavaScript
assignment `label(value)` → `label(value).toUpperCase()`. All six files match
expected hashes. Shared JS, SVG and PNG are unchanged. All six original link
and import spellings resolve to the original targets; no aliases remain and
no site files were added or removed.

## Verification

- Installed-path real-provider run: 54.066 seconds, 15 unique HTTPS attempts,
  all HTTP 200 with completed streams. Fourteen native calls succeed: eight
  reads, three directory listings and three edits. Final marker, Done state,
  orderly exit 0 and terminal restoration pass.
- One 1,175,731-byte trace has 1,392 contiguous events. Request/attempt IDs
  correlate all starts, responses and finishes, and saved bodies match the
  trace. The parent's expanded scan checks 39 private originals and 446 unique
  raw/encoded spellings across all available plaintext fields: zero leaks.
- Exact restoration/link/asset checks are in
  `scratch/FLT-313/provider-installed-1/restoration-audit.json`. Request coverage
  is in `request-correlation-audit.json`; capture/privacy limits are in
  `expanded-wire-audit.json`. `launch.json` and `startup-without-filter.txt`
  establish saved activation before first input.
- All 112 production configuration/session metadata entries match before/after.
  Fixture configuration, actual production configuration digest and installed
  binary SHA-256 `1390482d49bc2808cd4dafd1b74018d43436acf4f4c52e662e9cb9ec37eb2a4b`
  remain unchanged. No running user conversation was modified.

Independent read-only review passes all checks: 39 originals / 471 distinct
raw and encoded spellings, zero leaks, complete read results and exact model
argument replay. Receipt: `scratch/FLT-313/independent-audit.json`.

## Limits and retained evidence

The trace captures final application requests before HTTPS, not remote packets.
Ten encrypted reasoning replay fields in requests 6–15 are intentionally opaque;
there are no detected omissions in plaintext request fields. PNG pixels were
preserved by checksum and were not sent or visually tested. No browser or
visual-layout qualification is claimed.

An initial driver-generation quoting error was caught by syntax validation
before any provider call or production mutation. Its receipt is retained in
`scratch/FLT-313/preparation-failure.json`; the corrected driver ran once.

No product repair was necessary. No Rust sources, runtime settings, installed
binary or saved sessions changed; no release rebuild or reinstallation was
needed. FLT-312 focused checks and FLT-310 complete gate remain source evidence.
Changed documentation: this completed plan, FLT-312's follow-up link,
CONTINUITY.md, NEXT_STEPS.md, TESTING.md and docs/.INDEX.md. Test helpers and
receipts are under `scratch/FLT-313/`. Final evidence:
`scratch/FLT-313/final-assessment.json`.
