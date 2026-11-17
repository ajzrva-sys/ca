//! Occurrence journal. A fire builds its receipt before the run; reconcile applies it after restart.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Most one-shot occurrences retained across restarts. Further one-shots are refused.
pub const ONE_SHOT_CAP: usize = 8;
const RECEIPT_FILE: &str = "fire-receipts.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub id: String,
    pub one_shot: bool,
}

/// Receipt written before a run. `Malformed` is kept for quarantine and is not a fire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Receipt {
    Fire { id: String, one_shot: bool },
    Malformed { raw: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileStatus {
    Allow,
    Suppress,
    Quarantine,
    Refuse,
}

/// Pure receipt for `occurrence`. No clock and no I/O; the caller stores it before the run.
pub fn begin(occurrence: &Occurrence) -> Receipt {
    Receipt::Fire {
        id: occurrence.id.clone(),
        one_shot: occurrence.one_shot,
    }
}

/// After a restart, the same occurrence is suppressed. A malformed receipt is quarantined
/// and not applied. One-shots at `ONE_SHOT_CAP` are refused.
pub fn reconcile(receipts: &[Receipt], occurrence: &Occurrence) -> ReconcileStatus {
    let mut matched = false;
    let mut one_shots = 0usize;
    let mut quarantined = false;
    for receipt in receipts {
        match receipt {
            Receipt::Malformed { .. } => quarantined = true,
            Receipt::Fire { id, one_shot } => {
                if id == &occurrence.id {
                    matched = true;
                }
                if *one_shot {
                    one_shots = one_shots.saturating_add(1);
                }
            }
        }
    }
    if matched {
        return ReconcileStatus::Suppress;
    }
    if quarantined {
        return ReconcileStatus::Quarantine;
    }
    if occurrence.one_shot && one_shots >= ONE_SHOT_CAP {
        return ReconcileStatus::Refuse;
    }
    ReconcileStatus::Allow
}

/// Write a fire receipt under `dir` before the run. A later call with the same
/// occurrence, including after a restart that can still read the file, is suppressed.
/// A file that cannot be parsed is quarantined and left unchanged.
pub fn gate_fire(dir: &Path, occurrence: &Occurrence) -> ReconcileStatus {
    let path = dir.join(RECEIPT_FILE);
    let mut receipts = load_receipts(&path);
    let status = reconcile(&receipts, occurrence);
    if status != ReconcileStatus::Allow {
        return status;
    }
    receipts.push(begin(occurrence));
    match store_receipts(&path, &receipts) {
        Ok(()) => ReconcileStatus::Allow,
        Err(_) => ReconcileStatus::Refuse,
    }
}

fn load_receipts(path: &Path) -> Vec<Receipt> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            return vec![Receipt::Malformed {
                raw: error.to_string(),
            }]
        }
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        vec![Receipt::Malformed {
            raw: String::from_utf8_lossy(&bytes).chars().take(200).collect(),
        }]
    })
}

fn store_receipts(path: &Path, receipts: &[Receipt]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec(receipts).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occurrence(id: &str, one_shot: bool) -> Occurrence {
        Occurrence {
            id: id.to_string(),
            one_shot,
        }
    }

    #[test]
    fn the_same_occurrence_is_suppressed_the_second_time() {
        let first = occurrence("occ-a", false);
        let receipt = begin(&first);
        assert_eq!(reconcile(&[receipt], &first), ReconcileStatus::Suppress);
    }

    #[test]
    fn a_different_occurrence_is_allowed() {
        let first = occurrence("occ-a", false);
        let receipt = begin(&first);
        let other = occurrence("occ-b", false);
        assert_eq!(reconcile(&[receipt], &other), ReconcileStatus::Allow);
    }

    #[test]
    fn a_malformed_receipt_is_quarantined_and_not_applied() {
        let bad = Receipt::Malformed {
            raw: "not-a-receipt".into(),
        };
        let pending = occurrence("occ-a", false);
        let status = reconcile(&[bad], &pending);
        assert_eq!(status, ReconcileStatus::Quarantine);
        assert_ne!(status, ReconcileStatus::Suppress);
        assert_ne!(status, ReconcileStatus::Allow);
    }

    #[test]
    fn one_shots_past_the_cap_are_refused() {
        let receipts: Vec<Receipt> = (0..ONE_SHOT_CAP)
            .map(|index| begin(&occurrence(&format!("once-{index}"), true)))
            .collect();
        let extra = occurrence("once-extra", true);
        assert_eq!(reconcile(&receipts, &extra), ReconcileStatus::Refuse);
        let repeating = occurrence("weekly", false);
        assert_eq!(reconcile(&receipts, &repeating), ReconcileStatus::Allow);
    }
}
