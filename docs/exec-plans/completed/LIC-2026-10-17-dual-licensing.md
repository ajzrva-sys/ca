# Dual MIT/Apache-2.0 licensing

## Purpose and authorization

On October 17, 2026, the repository owner selected dual MIT/Apache-2.0 terms and
requested the changes, dependency notices, preservation of third-party licenses,
and explicit flags for ownership issues that cannot be independently verified.

## Scope

Base the patch on public commit 84a544d7932b167858146447bd09233b780825a6;
do not import newer local application work. Add canonical license texts,
workspace and fixture metadata, contribution/scope documentation, a reproducible
dependency legal-text inventory, and relevant upstream embedded-asset notices.
Preserve the original publication evidence and all third-party grants.

The historical `.agent/PLANS.md` is absent in this public snapshot. This record
uses the same purpose/scope/validation/outcome structure as the publication plan.

## Validation

- Cargo metadata resolves MIT OR Apache-2.0 for all six workspace packages.
  Metadata on disposable copies validates all thirteen standalone manifests.
- `cargo package --list --allow-dirty --offline` succeeds for all six workspace
  packages and includes both project license texts. Root packaging additionally
  includes all 1,536 required license/notice/tool/document paths checked.
- Apache text matches the canonical ASF text byte-for-byte; MIT terms match
  canonical SPDX text apart from wrapping and the project copyright notice.
  Member license files equal root copies.
- `python3 tools/generate_dependency_notices.py --check` passes offline. All
  650 registry archives match Cargo.lock checksums. The final inventory retains
  1,478 generated legal/embedded-notice files and 58 supplemental texts.
- Independent readback verifies 1,529 referenced text-path hashes, lock identity,
  source URLs/relative paths, and Python 3.9 syntax. An intentionally changed
  legal text in a disposable source copy makes `--check` fail with exactly one
  changed file, confirming drift detection.
- Changed/new guide and notice links resolve. Licensing metadata and new
  first-party documentation pass whitespace checks; copied original legal
  texts retain upstream formatting.
- Independent review found and corrected legal-filename omissions and a scoped
  primary-license classification issue. Available original notices, full terms,
  supplemental templates and unknown ownership remain distinct.

This patch changes licensing metadata, documentation and notice tooling, not
Rust behavior. Rust source, Cargo.lock, application version and installed
executable are unchanged. The Rust runtime suite/build/install was not rerun,
and no native binary linkage or ownership certification is implied.

## Outcome

The selected dual terms are implemented with preserved prior MIT permissions
and third-party exceptions. The dependency inventory is conservative across all
locked targets/build/dev resolution, not a bill of materials for a built binary.
Twenty-five packages lack a verified complete original primary license text;
canonical templates supplement declared terms without inventing ownership.
Earlier source-import ownership, Apple SDK-derived binding concerns and syntect
asset-generation provenance remain explicitly flagged in docs/LICENSING.md and
THIRD_PARTY_NOTICES.md. Source changes are prepared on a dedicated public-repo
branch for review; newer local application work is not part of this patch.
