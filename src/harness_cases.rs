//! Frozen coding inputs and host-owned verifier identity. Evaluation fixtures
//! are data; candidate edits never supply a command or success flag.
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{canonical_digest, HarnessCase, HarnessPlan};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

#[path = "../tests/reasoning_pilot_hidden/src/lib.rs"]
pub(crate) mod verifier;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenCodingCase {
    pub case: HarnessCase,
    pub fixture_id: String,
    pub files: BTreeMap<String, Vec<u8>>,
    pub allowed_paths: BTreeSet<String>,
}

impl FrozenCodingCase {
    pub fn freeze(
        source: &Path,
        fixture_id: &str,
        id: &str,
        predicted_fix: bool,
        held_out: bool,
    ) -> Result<Self> {
        if !verifier::FIXTURE_IDS.contains(&fixture_id) {
            bail!("unknown host verifier");
        }
        verifier::validate_fixture_source(source, fixture_id).map_err(anyhow::Error::msg)?;
        let files = read_files(source)?;
        let metadata: serde_json::Value = serde_json::from_slice(
            files
                .get("fixture.json")
                .context("missing fixture manifest")?,
        )?;
        let allowed_paths: BTreeSet<String> =
            serde_json::from_value(metadata["allowed_paths"].clone())?;
        let input_digest = canonical_digest(&files)?;
        let verifier_digest = verifier_digest(fixture_id)?;
        let grants_digest = grants_digest(&allowed_paths)?;
        let frozen = Self {
            case: HarnessCase {
                id: id.into(),
                input_digest,
                verifier_digest,
                grants_digest,
                held_out,
                predicted_fix,
            },
            fixture_id: fixture_id.into(),
            files,
            allowed_paths,
        };
        frozen.validate()?;
        Ok(frozen)
    }

    pub fn validate(&self) -> Result<()> {
        if !verifier::FIXTURE_IDS.contains(&self.fixture_id.as_str())
            || self.files.is_empty()
            || self.files.len() > 64
            || self.files.values().map(Vec::len).sum::<usize>() > 1024 * 1024
            || self.allowed_paths.is_empty()
            || self.files.keys().any(|name| !safe_path(name))
            || self
                .allowed_paths
                .iter()
                .any(|name| !safe_path(name) || !self.files.contains_key(name))
            || canonical_digest(&self.files)? != self.case.input_digest
            || verifier_digest(&self.fixture_id)? != self.case.verifier_digest
            || grants_digest(&self.allowed_paths)? != self.case.grants_digest
        {
            bail!("frozen coding case changed or exceeds its grants");
        }
        self.task()?;
        Ok(())
    }

    pub fn task(&self) -> Result<&str> {
        let task = std::str::from_utf8(self.files.get("TASK.md").context("missing task")?)?;
        if task.is_empty() || task.len() > 128 * 1024 {
            bail!("invalid task size");
        }
        Ok(task)
    }

    /// Destination must be a newly created host-owned directory. Files are
    /// created exclusively; preexisting contents are never overwritten.
    pub fn materialize(&self, destination: &Path) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        self.validate()?;
        std::fs::DirBuilder::new().mode(0o700).create(destination)?;
        for (name, bytes) in &self.files {
            let path = destination.join(name);
            std::fs::create_dir_all(path.parent().context("missing parent")?)?;
            std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)?
                .write_all(bytes)?;
        }
        if canonical_digest(&read_files(destination)?)? != self.case.input_digest {
            bail!("materialized fixture differs from frozen input");
        }
        Ok(())
    }

    /// Check the original files and reject extra files before running tests.
    /// Build products belong outside the trial workspace.
    pub fn check_edits(&self, workspace: &Path) -> Result<String> {
        canonical_digest(&self.result_files(workspace)?)
    }

    pub fn result_files(&self, workspace: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
        self.validate()?;
        let actual = read_files(workspace)?;
        self.validate_result_files(&actual)?;
        Ok(actual)
    }

    /// Apply identical edit grants to local snapshots and imported VM bytes.
    pub(crate) fn validate_result_files(&self, actual: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        self.validate()?;
        if actual.values().map(Vec::len).sum::<usize>() > 1024 * 1024 {
            bail!("candidate result exceeds its byte bound");
        }
        if actual.keys().ne(self.files.keys()) {
            bail!("candidate added or removed fixture files");
        }
        for (name, bytes) in actual {
            if !self.allowed_paths.contains(name) && self.files.get(name) != Some(bytes) {
                bail!("candidate changed protected fixture input");
            }
        }
        Ok(())
    }
}

pub fn freeze_reasoning_cases(repository: &Path) -> Result<Vec<FrozenCodingCase>> {
    verifier::FIXTURE_IDS
        .iter()
        .map(|id| {
            FrozenCodingCase::freeze(
                &repository.join("tests/fixtures/reasoning_pilot").join(id),
                id,
                id,
                false,
                false,
            )
        })
        .collect()
}

/// A production evaluation must carry the six regression fixtures plus a
/// separate target and two distinct, undisclosed held-out inputs.
pub fn validate_frozen_suite(plan: &HarnessPlan, cases: &[FrozenCodingCase]) -> Result<()> {
    plan.validate()?;
    if cases.len() != 9 || plan.cases.len() != cases.len() {
        bail!("evaluation requires nine frozen coding cases");
    }
    let mut ids = BTreeSet::new();
    for frozen in cases {
        frozen.validate()?;
        if !ids.insert(&frozen.case.id) || !plan.cases.contains(&frozen.case) {
            bail!("case is not bound to the frozen plan");
        }
    }
    for id in verifier::FIXTURE_IDS {
        if !cases.iter().any(|c| {
            c.case.id == id && c.fixture_id == id && !c.case.held_out && !c.case.predicted_fix
        }) {
            bail!("missing required reasoning regression fixture");
        }
    }
    let target: Vec<_> = cases
        .iter()
        .filter(|c| c.case.predicted_fix && !c.case.held_out)
        .collect();
    let held: Vec<_> = cases
        .iter()
        .filter(|c| c.case.held_out && !c.case.predicted_fix)
        .collect();
    if target.len() != 1
        || held.len() != 2
        || held[0].case.input_digest == held[1].case.input_digest
        || held
            .iter()
            .any(|c| c.case.input_digest == target[0].case.input_digest)
    {
        bail!("target and held-out cases must be distinct frozen inputs");
    }
    let implementation = |case: &FrozenCodingCase| -> Result<String> {
        canonical_digest(
            &case
                .allowed_paths
                .iter()
                .map(|name| (name, &case.files[name]))
                .collect::<BTreeMap<_, _>>(),
        )
    };
    let mut disclosed = cases
        .iter()
        .filter(|case| !case.case.held_out)
        .map(implementation)
        .collect::<Result<BTreeSet<_>>>()?;
    for case in held {
        if case.fixture_id != target[0].fixture_id || !disclosed.insert(implementation(case)?) {
            bail!("held-out variants must match the target family and have undisclosed implementations");
        }
    }
    Ok(())
}

fn verifier_digest(fixture_id: &str) -> Result<String> {
    canonical_digest(&(
        fixture_id,
        include_str!("../tests/reasoning_pilot_hidden/src/lib.rs"),
        env!("CA_BUILD_DIGEST"),
    ))
}
fn grants_digest(paths: &BTreeSet<String>) -> Result<String> {
    canonical_digest(&(
        "offline-workspace-only-v1",
        crate::harness_worker::CODING_TOOLS,
        paths,
    ))
}
fn safe_path(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
fn read_files(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut policy = crate::workspace_materialization::MaterializationPolicy::benchmark();
    policy.max_entries = 80;
    policy.max_logical_bytes = 1024 * 1024;
    policy.max_depth = 8;
    crate::workspace_materialization::read_bounded_regular_snapshot(root, &policy)?
        .into_iter()
        .map(|(name, bytes)| {
            Ok((
                name.to_str().context("non-UTF-8 fixture path")?.to_owned(),
                bytes,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap()
    }

    #[test]
    fn hrs253_all_six_regression_inputs_are_frozen_and_materialize_exactly() {
        let cases = freeze_reasoning_cases(&root()).unwrap();
        assert_eq!(cases.len(), 6);
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().canonicalize().unwrap();
        for case in cases {
            let path = destination.join(&case.case.id);
            case.materialize(&path).unwrap();
            assert_eq!(case.check_edits(&path).unwrap(), case.case.input_digest);
            assert!(case.materialize(&path).is_err());
            assert!(!case.task().unwrap().is_empty());
        }
    }

    #[test]
    fn hrs253_candidate_cannot_change_tests_or_redirect_fixture_reads() {
        let case = freeze_reasoning_cases(&root()).unwrap().remove(0);
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().canonicalize().unwrap();
        let path = directory.join("case");
        case.materialize(&path).unwrap();
        std::fs::write(path.join("src/lib.rs"), "pub fn replacement() {}\n").unwrap();
        assert_ne!(case.check_edits(&path).unwrap(), case.case.input_digest);
        std::fs::write(path.join("tests/public.rs"), "// skip checks").unwrap();
        assert!(case.check_edits(&path).is_err());
        std::fs::write(path.join("tests/public.rs"), &case.files["tests/public.rs"]).unwrap();
        let private = directory.join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("lib.rs"), "host-private-sentinel").unwrap();
        std::fs::rename(path.join("src"), path.join("old-src")).unwrap();
        std::os::unix::fs::symlink(&private, path.join("src")).unwrap();
        assert!(case.check_edits(&path).is_err());
    }

    #[test]
    fn hrs253_held_out_duplicates_and_missing_regressions_cannot_form_suite() {
        use codex_team::self_improvement::{
            HarnessSettings, SourceRepositorySnapshotV1, HARNESS_SCHEMA,
        };
        let mut cases = freeze_reasoning_cases(&root()).unwrap();
        let mut target = cases[0].clone();
        target.case.id = "target".into();
        target.case.predicted_fix = true;
        let mut held_a = cases[0].clone();
        held_a.case.id = "held-a".into();
        held_a.case.held_out = true;
        let mut held_b = cases[0].clone();
        held_b.case.id = "held-b".into();
        held_b.case.held_out = true;
        for (variant, implementation) in [
            (
                &mut held_a,
                b"pub fn truncate_utf8(input: &str, _: usize) -> String { input.to_owned() }\n"
                    .to_vec(),
            ),
            (
                &mut held_b,
                b"pub fn truncate_utf8(_: &str, _: usize) -> String { String::new() }\n".to_vec(),
            ),
        ] {
            variant.files.insert("src/lib.rs".into(), implementation);
            variant.case.input_digest = canonical_digest(&variant.files).unwrap();
        }
        cases.extend([target, held_a, held_b]);
        let mut plan = HarnessPlan {
            vm: None,
            preparation_seconds: None,
            guidance_source: None,
            activation_executable: None,
            source_files: Default::default(),
            schema: HARNESS_SCHEMA.into(),
            id: "suite".into(),
            baseline: SourceRepositorySnapshotV1 {
                schema: "test".into(),
                branch: "main".into(),
                head: "a".repeat(40),
                tracked_tree_digest: "a".repeat(64),
                submodule_digest: "a".repeat(64),
                repository_fingerprint: "a".repeat(64),
            },
            baseline_binary_digest: "a".repeat(64),
            route_digest: "b".repeat(64),
            price_digest: "c".repeat(64),
            evidence_ids: vec!["task:event".into()],
            cases: cases.iter().map(|c| c.case.clone()).collect(),
            repetitions: 3,
            settings: HarnessSettings::default(),
            digest: String::new(),
        };
        plan.seal().unwrap();
        validate_frozen_suite(&plan, &cases).unwrap();
        cases[8] = cases[7].clone();
        cases[8].case.id = "held-b".into();
        plan.cases = cases.iter().map(|c| c.case.clone()).collect();
        plan.seal().unwrap();
        assert!(validate_frozen_suite(&plan, &cases).is_err());
        cases.remove(0);
        plan.cases = cases.iter().map(|c| c.case.clone()).collect();
        plan.seal().unwrap();
        assert!(validate_frozen_suite(&plan, &cases).is_err());
    }
}
