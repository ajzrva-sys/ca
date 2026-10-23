//! In-process repository tools shared by CA and the compatibility executable.
//! The host supplies the root and authority; requests never select a root.

use super::{MechanisticExecutor, MechanisticReceiptV1, MechanisticRequestV1};
use crate::cancellation::CancellationToken;
use crate::paths::Paths;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct NativeCancellation(CancellationToken);

impl NativeCancellation {
    pub fn cancel(&self) {
        self.0.cancel();
    }
}

#[derive(Debug, Clone)]
pub struct NativeMechanisticService {
    root: PathBuf,
}

impl NativeMechanisticService {
    pub fn new(root: &Path) -> Result<Self> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            bail!("native repository tools require a directory");
        }
        let service = Self { root };
        service.revision()?;
        Ok(service)
    }

    pub fn revision(&self) -> Result<String> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        crate::revision::repository_revision(&Paths::new(&self.root)?)
    }

    pub fn tracked_paths(&self) -> Result<Vec<String>> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        super::query::tracked_files(&self.root)
    }

    pub fn require_projected_candidate_paths(&self, candidates: &[String]) -> Result<()> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        crate::verification::isolation::require_projected_candidate_paths(
            &Paths::new(&self.root)?,
            candidates,
        )
    }

    pub fn query(&self, request: &MechanisticRequestV1) -> Result<MechanisticReceiptV1> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        MechanisticExecutor::new(&self.root)?.execute_queries(request)
    }

    pub fn verify(
        &self,
        request: &MechanisticRequestV1,
        cancellation: &NativeCancellation,
    ) -> Result<MechanisticReceiptV1> {
        let _output = crate::machine_json::ProgressSuppressionGuard::enter();
        if request.actions.iter().any(|action| !action.is_evidence()) {
            bail!("native verification accepts evidence actions only");
        }
        MechanisticExecutor::new(&self.root)?
            .execute_evidence_with_cancellation(request, &cancellation.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mechanistic::{
        DelegationPolicy, MechanisticActionV1, MechanisticRequestPurposeV1,
        MECHANISTIC_REQUEST_SCHEMA,
    };
    use std::process::Command;

    #[test]
    fn native_queries_are_revision_bound_and_preserve_tracked_file_scope() {
        let temp = tempfile::tempdir().unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success());
        std::fs::write(temp.path().join("source.rs"), "fn main() {}\n").unwrap();
        std::fs::write(temp.path().join("draft.rs"), "draft\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "source.rs"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success());
        let service = NativeMechanisticService::new(temp.path()).unwrap();
        let mut request = MechanisticRequestV1 {
            schema: MECHANISTIC_REQUEST_SCHEMA.into(),
            expected_revision: service.revision().unwrap(),
            delegation_policy: DelegationPolicy::Require,
            purpose: MechanisticRequestPurposeV1::TransparentTool,
            allow_unisolated_evidence: false,
            actions: vec![MechanisticActionV1::ReadTrackedFile {
                path: "source.rs".into(),
                start_line: 1,
                max_lines: 20,
            }],
        };
        assert!(service.query(&request).is_ok());
        request.actions = vec![MechanisticActionV1::ReadTrackedFile {
            path: "draft.rs".into(),
            start_line: 1,
            max_lines: 20,
        }];
        assert!(service.query(&request).is_err());
        request.actions = vec![MechanisticActionV1::ReadTrackedFile {
            path: "source.rs".into(),
            start_line: 1,
            max_lines: 20,
        }];
        std::fs::write(temp.path().join("source.rs"), "fn changed() {}\n").unwrap();
        assert!(service.query(&request).is_err());
    }
}
