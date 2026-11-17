# FLT-308 — automatic HTML page-data privacy

Status: COMPLETE — signed ca 0.1.66 installed and interactively qualified.
Normal entry point: `ca --filter`, then edit an HTML
file in the ordinary conversation. No manually selected values or policy file
is required. The approved default hides page text, URLs, form values and
embedded business data, preserving tags, classes/IDs, CSS styling and script
logic. Explicit source rules override the automatic classification.

Use the existing tree-sitter analysis and reversible alias/edit pipeline.
Classify HTML content locally, including inline attributes and embedded
languages. Keep the unfiltered and strict-policy behavior intact. No new
provider tool, service, persisted policy format or UI is needed.

Verification: focused projection/restoration and boundary tests, format and
strict Clippy, independent combined-source review, real-provider HTML edit
with outgoing request capture. Then advance the patch once, build/install
one signed ca artifact recoverably and repeat the provider edit installed.
Record exact identities, actual outcomes and any limits here.

Implementation: HTML analysis collects page-data ranges independently of editor
selectors and applies Alias only as the public fallback. Existing explicit
rules take precedence; strict mode and unfiltered projections retain their
behavior. Existing native edits and registry aliases perform restoration.
The policy format and request/tool contracts are unchanged.

Review found and repaired HTML entity, CSS quoted import and dynamic selector
gaps. The first focused run (`scratch/check-change/runs/20261028T040217Z-34715`)
passed 11 checks; one new fixture assertion failed because its synthetic tokens
were prefixes of one another. Reversing that fixture's replacements fixes the
assertion; production registry restoration was not changed. The intermediate
source also had a test-only unused helper warning, fixed by retaining its
non-HTML test usage. Preserve that failed receipt.

Final source frozen at
`966fb940bbf329ae9595377c7c65440b383392179250985f87c695a07137ae17`.
Only three privacy Rust files and the standard root Cargo patch bump differ
from installed 0.1.65. Focused all-feature privacy checks, format, strict lint
and the single 0.1.66 release build are running. The existing FLT-307 complete
workspace gate remains the baseline for unchanged protocol/engine targets;
this bounded parser policy change does not change those interfaces or storage.

The second run (`20261028T040855Z-49343`) passed all 151 privacy checks
(two pre-existing opt-in performance measurements ignored) and formatting.
Strict Clippy rejected `end <= len - 1`; the equivalent `end < len` expression
is now independently re-reviewed. No release was built by the failed run.
Final focused receipt: `scratch/check-change/runs/20261028T041153Z-55998/report.json`.
All 151 privacy tests pass (two existing opt-in performance measurements
ignored), as do formatting and strict all-feature binary Clippy. The one
intended release build passed. The package was advanced once to 0.1.66.

`scratch/FLT-308/independent-review.json` approves the final source identity.
`release-source.json` records exact hashes and the five changed source files.
The signed artifact SHA-256 is
`4ca46b59a0d93945dce76e28b5c183cab71564824bbb189ba784bbb77de82937`.

Real-provider qualification: `provider-installed-2/receipt.json` passes the
ordinary interactive `ca --filter` entry at width 140 against native
openai-codex/gpt-6.1-sol. Five physical requests were captured with no trace
omissions. No fixture secret or checked raw/percent/base64/hex/HTML-entity/JSON
Unicode spelling appears. HTML structure, CSS and JavaScript remain readable.
The provider made exactly the requested color edit; all other local bytes,
including private data, are identical. No privacy policy file was needed.
Normal exit and terminal restoration pass. This task changes no TUI rendering;
the existing FLT-307 pending-update and narrow-layout proofs are unchanged.

Candidate-path requests twice failed to connect, including a tool-free new
candidate control; the previous qualified artifact could connect. Little Snitch
is active, but its exact rule was not inspected or changed. The exact same
signed artifact succeeds from the installed ca path; do not claim a general
candidate-path networking repair. Those failures remain preserved.

The first recoverable installation passed artifact readback but its interactive
harness retained headless-only `--trust`. Startup correctly rejected the flag;
the installer restored 0.1.65. The corrected harness removed that flag and the
second installation qualified successfully using the same artifact, without
another build. `install-receipt.json` preserves the rollback;
`install-receipt-2.json` records the successful installation and recovery copy.
Installed version/help/signature/hash/mode/ownership checks pass. The prior
0.1.65 remains recoverable. All 102 production config/session metadata entries
are unchanged (`runtime-after.json`); running sessions were left alone.

Scope limit: structural classes, IDs, object keys and executable identifiers
remain public unless explicitly selected. Structural masking is not semantic
classification of every possible secret embedded in executable code. The
provider test uses synthetic fixture data; it does not expose user page data.
