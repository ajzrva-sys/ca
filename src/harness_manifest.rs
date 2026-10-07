//! Evidence-gated harness change manifests — records WHY harness changes were made.
//!
//! Inspired by AHE's change_manifest.json pattern. Every harness modification
//! should be accompanied by: failure evidence, root cause, targeted fix, and
//! predicted impact. Legacy records are readable; matched experiments own new verdicts.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::harness_registry::HarnessComponent;
use crate::session_outcome::ComponentHash;
#[cfg(test)]
use crate::session_outcome::OutcomeKind;

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessChangeManifest {
    /// Schema version.
    pub version: u32,
    /// Unique ID for this manifest.
    pub id: String,
    /// Unix timestamp when the manifest was created.
    pub created_at: u64,
    /// The harness component that was changed.
    pub component: HarnessComponent,
    /// Hash of the component before the change.
    pub hash_before: ComponentHash,
    /// Hash of the component after the change.
    pub hash_after: ComponentHash,
    /// Reference to failure analysis reports that motivated the change.
    pub evidence_ids: Vec<String>,
    /// Root cause analysis — WHY the failure happened.
    pub root_cause: String,
    /// Description of the targeted fix.
    pub targeted_fix: String,
    /// Predicted impact on session outcomes.
    pub prediction: ChangePrediction,
    /// Optional ExecPlan reference.
    pub exec_plan: Option<String>,
    /// Verification result, filled in after enough sessions have passed.
    pub verification: Option<VerificationResult>,
}

#[allow(dead_code)]
const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePrediction {
    /// What metric is expected to change.
    pub metric: String,
    /// Direction: "improve" or "regress".
    pub direction: String,
    /// How many sessions to wait before verifying.
    pub verification_window: u32,
    /// Confidence level: "low", "medium", "high".
    pub confidence: String,
    /// Free-text rationale.
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// When verification was performed (Unix timestamp).
    pub verified_at: u64,
    /// Number of sessions observed.
    pub sessions_observed: u32,
    /// Outcome distribution before the change.
    pub outcomes_before: OutcomeDistribution,
    /// Outcome distribution after the change.
    pub outcomes_after: OutcomeDistribution,
    /// Verdict: "keep", "revert", "partial", "insufficient_data".
    pub verdict: String,
    /// Free-text notes.
    pub notes: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutcomeDistribution {
    #[serde(default)]
    pub unknown: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub abandoned: u32,
    pub completed: u32,
    pub failed: u32,
}

#[cfg(test)]
impl OutcomeDistribution {
    pub fn record(&mut self, outcome: OutcomeKind) {
        match outcome {
            OutcomeKind::Unknown => self.unknown += 1,
            OutcomeKind::Accepted => self.accepted += 1,
            OutcomeKind::Rejected => self.rejected += 1,
            OutcomeKind::Abandoned => self.abandoned += 1,
            OutcomeKind::Completed => self.completed += 1,
            OutcomeKind::Failed => self.failed += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.accepted + self.rejected + self.abandoned + self.completed + self.failed
    }

    pub fn positive_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.accepted as f64 / total as f64
    }
}

// ---------------------------------------------------------------------------
// Enforcement levels
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    /// Requires manifest for these components.
    Strict,
    /// Warns but allows changes.
    Advisory,
    /// No enforcement.
    Bypass,
}

#[allow(dead_code)]
pub fn enforcement_for(component: HarnessComponent) -> EnforcementMode {
    match component {
        HarnessComponent::AgentsMd
        | HarnessComponent::ToolDefinitions
        | HarnessComponent::SystemPrompt => EnforcementMode::Strict,
        HarnessComponent::ActiveSkills
        | HarnessComponent::MemoryStore
        | HarnessComponent::Config
        | HarnessComponent::Hooks => EnforcementMode::Advisory,
        _ => EnforcementMode::Bypass,
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub fn load_pending_manifests(data_dir: &Path) -> std::io::Result<Vec<HarnessChangeManifest>> {
    let dir = data_dir.join("manifests");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(m) = serde_json::from_str::<HarnessChangeManifest>(&data) {
                    if m.verification.is_none() {
                        manifests.push(m);
                    }
                }
            }
        }
    }
    Ok(manifests)
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

pub fn format_manifest(manifest: &HarnessChangeManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!("Manifest: {}\n", manifest.id));
    out.push_str(&format!("Component: {:?}\n", manifest.component));
    out.push_str(&format!(
        "Hash: {} -> {}\n",
        &manifest.hash_before[..12.min(manifest.hash_before.len())],
        &manifest.hash_after[..12.min(manifest.hash_after.len())]
    ));
    out.push_str(&format!("Root cause: {}\n", manifest.root_cause));
    out.push_str(&format!("Fix: {}\n", manifest.targeted_fix));
    out.push_str(&format!(
        "Prediction: {} ({}, confidence: {})\n",
        manifest.prediction.metric, manifest.prediction.direction, manifest.prediction.confidence
    ));
    if let Some(ref exec) = manifest.exec_plan {
        out.push_str(&format!("ExecPlan: {}\n", exec));
    }
    match &manifest.verification {
        Some(v) => out.push_str(&format!(
            "Verification: {} (verdict: {})\n",
            v.sessions_observed, v.verdict
        )),
        None => out.push_str("Verification: pending\n"),
    }
    out
}

#[allow(dead_code)]
fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_distribution_tracks_rates() {
        let mut dist = OutcomeDistribution::default();
        dist.record(OutcomeKind::Accepted);
        dist.record(OutcomeKind::Accepted);
        dist.record(OutcomeKind::Rejected);
        dist.record(OutcomeKind::Completed);
        assert_eq!(dist.total(), 4);
        assert!((dist.positive_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn enforcement_levels() {
        assert_eq!(
            enforcement_for(HarnessComponent::AgentsMd),
            EnforcementMode::Strict
        );
        assert_eq!(
            enforcement_for(HarnessComponent::ActiveSkills),
            EnforcementMode::Advisory
        );
        assert_eq!(
            enforcement_for(HarnessComponent::QualityGuardrails),
            EnforcementMode::Bypass
        );
    }
}
