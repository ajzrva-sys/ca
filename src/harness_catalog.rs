//! Host-owned, behaviorally distinct target/held-out implementations. These
//! variants are data; no candidate supplies verifiers or executable commands.
use crate::harness_cases::{freeze_reasoning_cases, FrozenCodingCase};
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::canonical_digest;
use std::path::Path;

fn variants(family: &str) -> Result<[&'static str; 3]> {
    Ok(match family {
        "unicode_parser_boundary" => [
            include_str!("harness_case_variants/unicode_parser_boundary/target.rs"),
            include_str!("harness_case_variants/unicode_parser_boundary/held_a.rs"),
            include_str!("harness_case_variants/unicode_parser_boundary/held_b.rs"),
        ],
        "permission_scope_matching" => [
            include_str!("harness_case_variants/permission_scope_matching/target.rs"),
            include_str!("harness_case_variants/permission_scope_matching/held_a.rs"),
            include_str!("harness_case_variants/permission_scope_matching/held_b.rs"),
        ],
        "protocol_tool_interleave" => [
            include_str!("harness_case_variants/protocol_tool_interleave/target.rs"),
            include_str!("harness_case_variants/protocol_tool_interleave/held_a.rs"),
            include_str!("harness_case_variants/protocol_tool_interleave/held_b.rs"),
        ],
        "atomic_journal_recovery" => [
            include_str!("harness_case_variants/atomic_journal_recovery/target.rs"),
            include_str!("harness_case_variants/atomic_journal_recovery/held_a.rs"),
            include_str!("harness_case_variants/atomic_journal_recovery/held_b.rs"),
        ],
        "process_timeout_cleanup" => [
            include_str!("harness_case_variants/process_timeout_cleanup/target.rs"),
            include_str!("harness_case_variants/process_timeout_cleanup/held_a.rs"),
            include_str!("harness_case_variants/process_timeout_cleanup/held_b.rs"),
        ],
        "config_migration" => [
            include_str!("harness_case_variants/config_migration/target.rs"),
            include_str!("harness_case_variants/config_migration/held_a.rs"),
            include_str!("harness_case_variants/config_migration/held_b.rs"),
        ],
        _ => bail!("no host-owned evaluation variants for this family"),
    })
}

/// Six fixed regressions, a target, and two undisclosed same-family variants.
pub(crate) fn automatic_cases(repository: &Path, family: &str) -> Result<Vec<FrozenCodingCase>> {
    let mut cases = freeze_reasoning_cases(repository)?;
    let baseline = cases
        .iter()
        .find(|case| case.fixture_id == family)
        .context("unsupported evaluation family")?
        .clone();
    for (index, implementation) in variants(family)?.into_iter().enumerate() {
        let mut case = baseline.clone();
        case.case.id = if index == 0 {
            "target".into()
        } else {
            format!("held-out-{}", index - 1)
        };
        case.case.predicted_fix = index == 0;
        case.case.held_out = index != 0;
        if case.allowed_paths.len() != 1 || !case.allowed_paths.contains("src/lib.rs") {
            bail!("automatic family grants changed");
        }
        case.files
            .insert("src/lib.rs".into(), implementation.as_bytes().to_vec());
        case.case.input_digest = canonical_digest(&case.files)?;
        case.validate()?;
        cases.push(case);
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hrs253_automatic_catalog_seals_distinct_same_family_cases() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for family in crate::harness_cases::verifier::FIXTURE_IDS {
            let cases = automatic_cases(root, family).unwrap();
            let (mut plan, _, _) = crate::harness_generation::tests::inputs();
            plan.cases = cases.iter().map(|case| case.case.clone()).collect();
            plan.seal().unwrap();
            crate::harness_cases::validate_frozen_suite(&plan, &cases).unwrap();
            assert_eq!(cases.len(), 9);
        }
        assert!(automatic_cases(root, "unclassified").is_err());
    }

    #[tokio::test]
    #[ignore = "requires fresh CA_HARNESS_TEST_BINARY and macOS sandbox; verifies all 18 automatic variants"]
    async fn hrs253_automatic_variants_fail_before_repair_and_pass_after_repair() {
        let binary = std::path::PathBuf::from(
            std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh worker binary"),
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap();
        for family in crate::harness_cases::verifier::FIXTURE_IDS {
            for case in automatic_cases(&root, family).unwrap().into_iter().skip(6) {
                let temp = tempfile::tempdir().unwrap();
                let workspace = temp.path().canonicalize().unwrap().join("trial");
                case.materialize(&workspace).unwrap();
                let broken = crate::harness_verifier::verify_coding_workspace(
                    &binary,
                    &root,
                    &workspace,
                    &case,
                    std::time::Duration::from_secs(60),
                )
                .await
                .unwrap();
                assert!(
                    broken.public_passed && !broken.hidden_passed,
                    "{family}/{}: {broken:?}",
                    case.case.id
                );
                std::fs::copy(
                    root.join("tests/reasoning_pilot_golden")
                        .join(family)
                        .join("src/lib.rs"),
                    workspace.join("src/lib.rs"),
                )
                .unwrap();
                let repaired = crate::harness_verifier::verify_coding_workspace(
                    &binary,
                    &root,
                    &workspace,
                    &case,
                    std::time::Duration::from_secs(60),
                )
                .await
                .unwrap();
                assert!(
                    repaired.public_passed && repaired.hidden_passed,
                    "{family}/{}: {repaired:?}",
                    case.case.id
                );
            }
        }
    }
}
