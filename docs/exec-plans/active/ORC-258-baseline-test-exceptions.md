# ORC-258 baseline test exceptions

The unfiltered fresh default workspace run at 2026-10-05 23:01 UTC is **not
green**. Four ca failures led to corrected Plan/comment fixtures and compact
usage accounting. Nine separate codex-team integration failures below are
pre-existing mismatches between test contracts and exposed CLI/MCP surfaces.
They remain failures in the unfiltered run, not passing tests.

Source was compared with
`scratch/oracle-improvements/20261005T211220Z/source-manifest.json` and the
recoverable `source-baseline.tar.gz`. The fresh workspace build eliminated the
old nested-checkout artifacts. Production files below are byte-identical to
the baseline:

| Source | SHA-256 |
| --- | --- |
| `crates/codex-team/src/cli.rs` | `c2dac1bae51d680cd0066f08b2aca88fff22358be5e56c5fd6e1f4d65e68f5bd` |
| `crates/codex-team/src/main.rs` | `7630ef61ebf46ed233486192e3e96ca947d9936894f184db6c639719766c82c5` |
| `crates/codex-team/src/mcp/server.rs` | `f924f4aa2ba760dd2e503a06ca61751d642397d797a925c68267cada9fdcb1a2` |
| `crates/codex-team/src/lib.rs` | `d7bc44c3618ea8a57baab4262c21cb5243dd0e487f69ed5ccf4b536d63aaabab` |
| `crates/codex-team-protocol/src/lib.rs` | `abbafa8da4502c54106de14946abc9c3c43ebe9f1f464054736fb4b0a8687657` |

Test command arguments and assertions are unchanged from the baseline; only
their stale binary resolution was repaired. Isolated runs reproduced these
failures against freshly built current binaries.

| Test | Existing gap |
| --- | --- |
| `five_stdio_clients_share_evidence_cache_and_last_consumer_cancellation` | `mcp --access delegate` is rejected; only `observe` and `operate` are exposed. |
| `five_stdio_clients_share_one_query_and_persistent_receipt` | Same missing delegate surface. |
| `real_stdio_delegate_is_observe_plus_durable_delegation` | Same missing delegate surface. |
| `real_stdio_observe_negotiates_exact_tools_and_zero_resources` | Observe does not expose the expected `repo_query` tool. |
| `real_stdio_operate_is_explicit_and_closed_schema` | Operate does not expose the expected `repo_query` tool. |
| `typed_remote_cli_negotiates_bounded_read_only_transmission_rpc` | The parser does not expose `remote`. |
| `protected_baseline_cli_excludes_only_exact_finder_metadata_names` | The parser does not expose `protected-baseline`. |
| `protected_baseline_cli_is_digest_bound_head_anchored_and_mutation_free` | The legacy protected-tree checker does not emit the expected missing-baseline receipt; the baseline implementation is not registered and its CLI is also absent. |
| `protected_baseline_cli_rejects_stale_revision_without_writing` | Same missing protected-baseline surface. |

The applicable default/all-feature matrices exclude exactly these nine names
with explicit Rust test `--skip` arguments. Their receipts must distinguish
these exclusions from the existing ignored manual/credentialed tests. The
unfiltered log is retained at
`/private/tmp/ca-orc258-final-default_workspace_tests.log`.
Strict Clippy, formatting and the all-feature release build still cover the
entire workspace. No unrelated codex-team production commands were added to
make these tests pass.
