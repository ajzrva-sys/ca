# FLT-274 Mechanistic provider filter

## Purpose

Give the user a deterministic gate between ca and every provider. The user
writes the rules. A trip stops the session and all provider communication.

## Behavior

- Config `[provider_filter]` holds `enabled`, an optional absolute `trip_file`,
  a `phrases` list, a `urls` list, up to 32 `adjustments`, an `obfuscate`
  switch, and up to 32
  `literal` or `regex` rules aimed at `request`, `response`, or `both`.
  Phrases and URLs count toward the rule cap. Adjustments have their own cap.
  A phrase is a case-insensitive literal in either direction:
  `"A-123"` matches `A-123`, `a-123`, and the same text inside quotation
  marks. The trip names `phrase-1`, `phrase-2`, and so on, never the phrase.
  A URL is an absolute `http` or `https` URL. `http://stop.com` matches that
  host at the root. `http://stop.com/stop` matches that path. Neither matches
  `http://anything.else`, a lookalike host, or a different path. The trip
  names `url-1`, `url-2`, and so on, never the URL. A named rule `pattern`
  accepts the same text.
- Request and `both` rules scan every byte gathered for a provider before
  omission or send: file reads, tool and command output, attachments, MCP
  text, and the serialized request. Response rules scan provider response
  bytes. The optional trip file is polled;
  exact trimmed contents `stop` trips it. A missing file does not. A symlink
  or unreadable file fails closed.
- The trip is process-sticky. Retries, fallbacks, new turns, subagent provider
  calls, and in-flight streams stop. Interactive status does not offer retry.
  Headless and service exit 11 with kind `provider_filter`.
- `adjustments` rewrite outbound text before a provider sees it, including
  text inside a file or page. `stop.txt` becomes `go.txt`. A literal keyword
  is changed in every detected casing, so `Stop.txt` and `STOP.TXT` become
  `go.txt` as well. An `http` or `https` `from` is a URL: `http://stop.com`
  becomes `http://go.com`, and a longer path keeps its suffix.
  `http://stop.com/stop` also rewrites paths under it.
  `http://anything.else` and lookalike hosts stay. Tools still open the real
  file and fetch the real URL. A reply is scanned as sent, then stand-ins
  are restored in the transcript. Request stop rules
  see the rewritten bytes. The same adjustment does not scan its own
  replacement. `enabled = false` does not adjust.
- `obfuscate = true` rewrites each phrase to `ca-kw-NN` and each URL to
  `http://ca-rNN.invalid` or `https://ca-rNN.invalid`, keeping a longer path's
  suffix. An explicit adjustment for the same keyword or URL is used instead.
  Fetched HTML source is kept so tags and links are rewritten. Disk names and
  bytes stay as written. Tool arguments are mapped back to the original file
  name or URL before the tool opens them. Request stop rules see the rewritten
  bytes. A reply is scanned as sent, then stand-ins are restored to the
  configured original in the transcript. The next request is adjusted again
  before it leaves.
- `page_stand_in` rewrites words in a `web_fetch` page before the provider
  sees that page. `latin` uses a built-in Latin list. `characters`, `words`,
  and `phrases` use `page_stand_ins`. The same spelling always becomes the
  same `«…»` token, a base-N encoding of that word's bytes plus a checksum.
  Tag names and URLs stay. The user's message and other tool output stay.
  The fetch still uses the real URL. Disk bytes stay. A reply is scanned as
  sent, then the tokens are restored. An explicit adjustment still wins for
  that keyword. A stop phrase in the fetched page is still judged on the
  page itself. `off` and `enabled = false` do not rewrite. An enabled page
  stand-in shows the FILTER ON banner and keeps fetched HTML source.
- `placeholder_images = true` replaces pictures in the outbound copy with
  the text `[image]`. Chat `image_url`, responses `input_image`, and
  Anthropic `image` blocks become text blocks. An `<img>` tag, an `<image>`
  tag, a markdown image, and a `data:image` URL in page text do too. A
  normal `https` URL stays. The picture on screen, the file on disk, and
  the fetch stay real. The placeholder stays text. The request is judged
  after the replacement. A fetched page is still judged on the page itself.
  `enabled = false` leaves images as they are. With the switch on, the
  FILTER ON banner is on and a fetched page keeps its HTML source.
- `obfuscation_patterns` rewrites each word in the outbound copy and does
  not trip. `secret` becomes `ca-kw-01`. A kill-switch phrase can be absent.
  The same word in both lists is rewritten on the way out, and a reply that
  still contains the word still trips. An explicit adjustment wins. The list
  has its own cap of 32. `enabled = false` does not rewrite.
- While the filter is active, including an adjustment-only filter, the TUI
  keeps a full-width top banner that says FILTER ON and nothing else. It stays
  above the conversation and open panels.
- After a request is prepared, a side pane shows that provider-bound body.
  Fetched page text comes first. The pane is framed with the same classified
  banner style, labeled TO PROVIDER. A terminal narrower than 80 columns
  hides the pane.
- The reported reason is the rule id or `trip file`, never the matched bytes.
- The first enabled snapshot sticks until the process exits.

## Validation

2026-10-11: `cargo test --bin ca -- provider_filter turn_recovery::tests::permanent_and_ambiguous -- --test-threads=8` passed 9 tests. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-12: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 13 tests, including case-insensitive quoted phrases such as `A-123`. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-12: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 15 tests, including exact `http://stop.com` and `http://stop.com/stop` matches and the `http://anything.else` non-match. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-12: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 19 tests, including `stop.txt` to `go.txt`, `http://stop.com` to `http://go.com` with path suffixes, lookalike hosts left unchanged, and stop rules judging the rewritten request. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 19 tests. A literal keyword is now changed in every detected casing before the provider sees the request, so `Stop.txt` and `STOP.TXT` follow `stop.txt`. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 20 tests. `obfuscate = true` rewrote a local HTML page, a URL, file text, and a file name to `ca-kw-NN` and `http://ca-r01.invalid`, left the file on disk unchanged, and restored the stand-in to the real name. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 21 tests. Phrase `redactme` with `obfuscate = true` rewrote every `redactme` and `REDACTME` in `tests/fixtures/provider-filter/redactme.html`, `redactme.txt`, and `redactme.js` to `ca-kw-01`, including the file names, and left `redacted` and the on-disk bytes unchanged. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 22 tests. Explicit adjustment `redactme` → `REDACTED` rewrote every casing in those fixtures to `REDACTED` and won over `ca-kw-01`. The existing word `redacted` stayed on the way out. The reverse pass mapped that word back to `redactme`. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 23 tests. A provider echo of `REDACTED`, including 5-byte chunks, is shown as `redactme` in the HTML, text, and JS fixtures. `ca-kw-01` leaves `Leave redacted alone.` unchanged. `cargo test --bin ca -- agent::stream::tests -- --test-threads=8` passed 18 tests. No live provider. Full `./scripts/verify.sh` was not run.

2026-10-13: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 25 tests. `page_stand_in = "latin"` rewrote words in a `web_fetch` page into stable `«…»` Latin tokens, left the user message, a `read_file` result, tag names, and URLs in place, and restored the original spelling, including a token split across chunks. Character, word, and phrase alphabets round-tripped. `off` and `enabled = false` did not rewrite. `redactme` → `REDACTED` stayed inside the page and came back as `redactme`. The `redactme.html` fixture on disk was unchanged. No live provider. Full `./scripts/verify.sh` was not run. `cargo fmt --all -- --check` passed. Strict clippy still fails on pre-existing dead code.

2026-10-14: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 27 tests. `placeholder_images = true` replaced chat `image_url`, responses `input_image`, and Anthropic `image` blocks, plus `<img>`, `<image>`, markdown images, and `data:image` URLs, with `[image]`. A normal `https` URL stayed. A phrase that lived only inside an image did not trip the request. `enabled = false` and `placeholder_images = false` left the image bytes in place. A second pass stayed stable. No live provider. Full `./scripts/verify.sh` was not run. `cargo fmt --all -- --check` passed. `cargo clippy --bin ca -- -D warnings -A dead_code --no-deps` still fails only on the pre-existing ASCII case comparison in `contains_literal`.

2026-10-14: `cargo test --bin ca -- provider_filter::tests -- --test-threads=8` passed 28 tests. `obfuscation_patterns = ["secret"]` rewrote `SECRET` to `ca-kw-01` and did not trip on the request or the reply. A phrase that was not in that list still tripped and was left as written. The same word in both lists was rewritten on the way out and still tripped in a reply. An explicit adjustment won. `enabled = false` left the word in place. No live provider. Full `./scripts/verify.sh` was not run. `cargo fmt --all -- --check` passed. `cargo clippy --bin ca -- -D warnings -A dead_code --no-deps` still fails only on the pre-existing ASCII case comparison in `contains_literal`.
