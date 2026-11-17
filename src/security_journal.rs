//! Security records reuse Prepared, Running, Confirmed, and Unknown.
//! They store references and metadata, not secrets or transcript bodies.
#![allow(dead_code)]
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    Prepared,
    Running,
    Confirmed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityRecord {
    pub version: u32,
    pub sequence: u64,
    pub previous_digest: String,
    pub action_id: String,
    pub grant_digest: String,
    pub operation_digest: String,
    pub state: EffectState,
    pub intent: String,
    pub outcome: Option<String>,
}

#[derive(Debug)]
pub struct SecurityJournal {
    path: PathBuf,
    sequence: u64,
    previous_digest: String,
}

impl SecurityJournal {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        Ok(Self {
            path,
            sequence: 0,
            previous_digest: "genesis".into(),
        })
    }

    pub fn prepare(
        &mut self,
        action_id: &str,
        grant_digest: &str,
        operation_digest: &str,
        intent: &str,
    ) -> Result<SecurityRecord, String> {
        if action_id.is_empty() || intent.is_empty() {
            return Err("intent publication failed".into());
        }
        self.append(
            action_id,
            grant_digest,
            operation_digest,
            EffectState::Prepared,
            intent,
            None,
        )
    }

    pub fn begin(&mut self, prepared: &SecurityRecord) -> Result<SecurityRecord, String> {
        if prepared.state != EffectState::Prepared {
            return Err("action is not prepared".into());
        }
        self.append(
            &prepared.action_id,
            &prepared.grant_digest,
            &prepared.operation_digest,
            EffectState::Running,
            &prepared.intent,
            None,
        )
    }

    pub fn confirm(
        &mut self,
        running: &SecurityRecord,
        outcome: &str,
    ) -> Result<SecurityRecord, String> {
        if running.state != EffectState::Running {
            return Err("action is not running".into());
        }
        self.append(
            &running.action_id,
            &running.grant_digest,
            &running.operation_digest,
            EffectState::Confirmed,
            &running.intent,
            Some(outcome),
        )
    }

    pub fn mark_unknown(&mut self, running: &SecurityRecord) -> Result<SecurityRecord, String> {
        self.append(
            &running.action_id,
            &running.grant_digest,
            &running.operation_digest,
            EffectState::Unknown,
            &running.intent,
            None,
        )
    }

    fn append(
        &mut self,
        action_id: &str,
        grant_digest: &str,
        operation_digest: &str,
        state: EffectState,
        intent: &str,
        outcome: Option<&str>,
    ) -> Result<SecurityRecord, String> {
        self.sequence = self.sequence.saturating_add(1);
        let record = SecurityRecord {
            version: 1,
            sequence: self.sequence,
            previous_digest: self.previous_digest.clone(),
            action_id: action_id.into(),
            grant_digest: grant_digest.into(),
            operation_digest: operation_digest.into(),
            state,
            intent: intent.into(),
            outcome: outcome.map(str::to_string),
        };
        let line = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        self.previous_digest = format!("{:x}", sha2::Sha256::digest(line.as_bytes()));
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(record)
    }

    pub fn records(&self) -> Result<Vec<SecurityRecord>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&self.path).map_err(|error| error.to_string())?;
        text.lines()
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_str(line).map_err(|error| error.to_string()))
            .collect()
    }
}

pub fn publication_blocks_dispatch(published: bool) -> bool {
    !published
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_publication_does_not_repeat_a_seen_effect() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = SecurityJournal::open(root.path().join("security.jsonl")).unwrap();
        assert!(publication_blocks_dispatch(false));
        let prepared = journal
            .prepare("action-1", "grant", "operation", "create artifact")
            .unwrap();
        let running = journal.begin(&prepared).unwrap();
        let unknown = journal.mark_unknown(&running).unwrap();
        assert_eq!(unknown.state, EffectState::Unknown);
        let records = journal.records().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .filter(|record| record.state == EffectState::Confirmed)
                .count(),
            0
        );
        let confirmed = journal.confirm(&running, "observed-once").unwrap();
        assert_eq!(confirmed.outcome.as_deref(), Some("observed-once"));
        assert!(journal
            .prepare("", "grant", "operation", "create artifact")
            .is_err());
    }
}
