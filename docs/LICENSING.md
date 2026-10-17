# Licensing

On October 17, 2026, the project selected dual licensing for CA-owned material:
[MIT](../LICENSE-MIT) **OR** [Apache-2.0](../LICENSE-APACHE), at your option.
This covers the root application and all five workspace crates, including
CA-owned documentation, prompts, schemas, tests, tools, and screenshots.
Separate third-party terms and notices retain their own scope.

## Reuse and contributions

Either option permits commercial use, modification, private forks, and
redistribution, including incorporation into proprietary products. Neither
requires publishing modifications. Comply with the license you choose:

- MIT requires retaining its copyright and permission notice in copies or
  substantial portions of the software.
- Apache-2.0 requires its license text, applicable existing notices, prominent
  notices on modified files, and any applicable upstream NOTICE attribution.
  It includes an express contributor patent grant for qualifying claims and
  terminates that grant in the patent-litigation circumstances stated in the
  license. It does not grant trademark rights.

The five member crates previously declared MIT. Those existing MIT permissions
remain available. This selection does not create an Apache patent grant from
an author who supplied material only under MIT, or expand anyone's authority
over another party's work. Incoming original contributions are offered under
both options; recipients choose either. See [CONTRIBUTING](../CONTRIBUTING.md).

The October 6 publication record describes the licensing status at that time.
The October 17 selection supplies the project-wide grant for covered material;
it does not change third-party terms or establish earlier ownership.

## Dependencies and distributions

[THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md) describes dependency obligations
and the release-notice workflow. The
[dependency inventory](../licenses/dependency-inventory.json) records the
audited versions and metadata. The
[notice generator](../tools/generate_dependency_notices.py) prepares notices
from the resolved dependency set and available upstream license files.
Dependency metadata alone is not a substitute for the required license texts,
copyright notices, or upstream NOTICE files in a distribution.

Downloaded models, external runtimes, subprocess programs, plugins, and other
separately obtained components keep their own terms. Review the exact contents
of each source or binary release; the CA license does not relicense them.

## Provenance record and remaining uncertainty

The bounded public-source audit examined commit
`84a544d7932b167858146447bd09233b780825a6`. It identified no specific external
source component with an unknown license. It did not independently establish
the ownership of every input to the initial source import:

- The initial public import began from a source snapshot without earlier Git
  history ([publication record](PUBLICATION_VALIDATION.md)). Earlier authorship,
  employer rights, and permissions for any prior contributions could not be
  reconstructed from that history. Resolve a concrete ownership or imported
  material claim from its original author and source records when identified.
- The repository describes behavior learned from Cursor, Codex, OpenCode,
  MiMo, Hermes, and Grok. No copied oracle implementation was identified in the
  bounded inventory. `oracles/` and `reference/` are excluded from the public
  tree. These findings are not an independent clean-room or authorship
  certification; see [PARITY](../PARITY.md).
- The [five screenshot captures](images/capture-receipt.json) show CA output
  using a disposable fixture provider. The capture tool uses a locally
  installed font; no font software is bundled. Historical HRS-253 records
  reference a modified external guest-agent and its retained license. That
  agent, source patch, and binary are not in this public source tree. Review
  its upstream terms and notices before including it in a future distribution.
- Upstream [objc2](../licenses/third-party/upstream/objc2-0.6.4/LICENSE.md)
  and [dispatch2](../licenses/third-party/upstream/dispatch2-0.3.1/LICENSE.md)
  notices explicitly discuss uncertainty about Apple SDK-derived bindings and
  redistribution under Xcode terms. Their original statements are preserved;
  this audit has not resolved that rights question. Rust license metadata and
  CA's license selection do not establish permission from Apple.
- Some dependency archives omit original license or attribution files.
  [Third-party notices](../THIRD_PARTY_NOTICES.md) distinguishes recovered
  upstream notices, supplemental canonical license terms, and unresolved
  original-notice gaps. Syntect's serialized syntax/theme assets also lack a
  reproducible manifest proving their precise generation inputs; preserved
  upstream asset notices do not establish that missing provenance.

This is an explicit record of verification limits, not an exception withholding
the selected licenses from CA-owned files. Preserve any established third-party
notices and investigate a specific provenance issue before claiming authority
to offer that material under additional terms.
