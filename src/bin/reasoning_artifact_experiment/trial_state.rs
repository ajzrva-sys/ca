//! Immutable per-trial controller state for RND-097.
//!
//! The state journal deliberately records phase boundaries, not provider or
//! tool bodies.  In particular, `execution_started` is durable before a
//! controller may ask a broker to make a call; a later `interrupted` state is
//! terminal and therefore cannot be accidentally retried by `resume`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TRIAL_STATE_SCHEMA_V1: &str = "ca.rnd097.trial_state.v1";
const JOURNAL_DIRECTORY: &str = "trial-state-v1";
const MAX_EVENTS: usize = 10_000;
const MAX_EVENT_BYTES: usize = 32 * 1024;
const EVENT_HASH_DOMAIN: &[u8] = b"ca-rnd097-trial-state-v1\0";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrialPhaseV1 {
    FixtureReady,
    WorkspaceMaterialized,
    ExecutionStarted,
    RecoveryExported,
    RecoveryProven,
    VerifierCompleted,
    Settled,
    Failed,
    Interrupted,
}

impl TrialPhaseV1 {
    fn terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Failed | Self::Interrupted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TrialStateEventV1 {
    pub schema_version: String,
    pub sequence: u64,
    pub previous_event_sha256: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub phase: TrialPhaseV1,
    pub occurred_at: String,
    pub workspace_id: Option<String>,
    pub recovery_artifact_sha256: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TrialStateFileV1 {
    event: TrialStateEventV1,
    event_sha256: String,
}

pub struct TrialStateJournal {
    root: PathBuf,
    preregistration_sha256: String,
}

impl std::fmt::Debug for TrialStateJournal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrialStateJournal")
            .field("root", &"[PRIVATE]")
            .field("preregistration_sha256", &self.preregistration_sha256)
            .finish()
    }
}

impl TrialStateJournal {
    /// Opens one owner-only journal beneath an already owner-only experiment
    /// directory.  It does not create or load providers, tools, sessions, or
    /// workspace state.
    pub fn create(experiment_dir: &Path, preregistration_sha256: &str) -> Result<Self> {
        require_unix()?;
        validate_sha256(preregistration_sha256)?;
        let experiment_dir = experiment_dir
            .canonicalize()
            .context("cannot canonicalize experiment directory")?;
        require_owner_directory(&experiment_dir)?;
        let root = experiment_dir.join(JOURNAL_DIRECTORY);
        match fs::symlink_metadata(&root) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                bail!("trial state journal root is invalid")
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&root)?;
                #[cfg(unix)]
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
            }
            Err(error) => return Err(error.into()),
        }
        let root = root.canonicalize()?;
        require_owner_directory(&root)?;
        Ok(Self {
            root,
            preregistration_sha256: preregistration_sha256.into(),
        })
    }

    /// Opens an existing journal without creating any filesystem state.  Read
    /// paths such as `report` and the preflight half of `resume` must not make
    /// a failed or unstarted pilot appear to have execution state.
    pub fn open_existing(experiment_dir: &Path, preregistration_sha256: &str) -> Result<Self> {
        require_unix()?;
        validate_sha256(preregistration_sha256)?;
        let experiment_dir = experiment_dir
            .canonicalize()
            .context("cannot canonicalize experiment directory")?;
        require_owner_directory(&experiment_dir)?;
        let root = experiment_dir.join(JOURNAL_DIRECTORY);
        let metadata = fs::symlink_metadata(&root)
            .with_context(|| format!("trial state journal does not exist at {}", root.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("trial state journal root is invalid");
        }
        let root = root.canonicalize()?;
        require_owner_directory(&root)?;
        Ok(Self {
            root,
            preregistration_sha256: preregistration_sha256.into(),
        })
    }

    /// Reports whether a journal exists while enforcing the same ownership and
    /// no-symlink requirements as an open.  It is intentionally not a blind
    /// `Path::exists` check so a malicious replacement cannot be mistaken for
    /// an unstarted trial.
    pub fn exists(experiment_dir: &Path) -> Result<bool> {
        require_unix()?;
        let experiment_dir = experiment_dir
            .canonicalize()
            .context("cannot canonicalize experiment directory")?;
        require_owner_directory(&experiment_dir)?;
        let root = experiment_dir.join(JOURNAL_DIRECTORY);
        match fs::symlink_metadata(&root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("trial state journal root is invalid");
                }
                let root = root.canonicalize()?;
                require_owner_directory(&root)?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn append(
        &self,
        trial_id: &str,
        slot_index: u16,
        phase: TrialPhaseV1,
        workspace_id: Option<&str>,
        recovery_artifact_sha256: Option<&str>,
        reason_code: Option<&str>,
    ) -> Result<TrialStateEventV1> {
        validate_trial_id(trial_id)?;
        validate_optional_id("workspace_id", workspace_id, 128)?;
        validate_optional_sha256(recovery_artifact_sha256)?;
        validate_optional_id("reason_code", reason_code, 128)?;
        let events = self.load_all()?;
        validate_existing_chain(&events, &self.preregistration_sha256)?;
        let previous = events.last().map(|file| file.event_sha256.as_str());
        let trial_previous = events
            .iter()
            .rev()
            .find(|file| file.event.trial_id == trial_id)
            .map(|file| file.event.phase);
        validate_transition(trial_previous, phase)?;
        let event = TrialStateEventV1 {
            schema_version: TRIAL_STATE_SCHEMA_V1.into(),
            sequence: events.len() as u64,
            previous_event_sha256: previous.unwrap_or(&"0".repeat(64)).to_string(),
            preregistration_sha256: self.preregistration_sha256.clone(),
            trial_id: trial_id.into(),
            slot_index,
            phase,
            occurred_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            workspace_id: workspace_id.map(str::to_owned),
            recovery_artifact_sha256: recovery_artifact_sha256.map(str::to_owned),
            reason_code: reason_code.map(str::to_owned),
        };
        validate_event(&event, &self.preregistration_sha256)?;
        let event_sha256 = hash_event(&event)?;
        let bytes = serde_json::to_vec(&TrialStateFileV1 {
            event: event.clone(),
            event_sha256,
        })?;
        if bytes.len() > MAX_EVENT_BYTES {
            bail!("trial state event exceeds the bound");
        }
        self.write_event(event.sequence, &bytes)?;
        Ok(event)
    }

    pub fn events(&self) -> Result<Vec<TrialStateEventV1>> {
        let events = self.load_all()?;
        validate_existing_chain(&events, &self.preregistration_sha256)?;
        Ok(events.into_iter().map(|file| file.event).collect())
    }

    fn load_all(&self) -> Result<Vec<TrialStateFileV1>> {
        let mut paths = fs::read_dir(&self.root)?.collect::<io::Result<Vec<_>>>()?;
        paths.sort_by_key(|entry| entry.file_name());
        if paths.len() > MAX_EVENTS {
            bail!("trial state journal exceeds event bound");
        }
        let mut events = Vec::with_capacity(paths.len());
        for (expected, entry) in paths.into_iter().enumerate() {
            let expected_name = format!("{expected:020}.json");
            if entry.file_name() != std::ffi::OsString::from(expected_name) {
                bail!("trial state journal has an invalid event sequence");
            }
            events.push(read_owner_event(&entry.path())?);
        }
        Ok(events)
    }

    fn write_event(&self, sequence: u64, bytes: &[u8]) -> Result<()> {
        let final_path = self.root.join(format!("{sequence:020}.json"));
        let temporary = self.root.join(format!(".{sequence:020}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = fs::hard_link(&temporary, &final_path) {
            let _ = fs::remove_file(&temporary);
            return Err(anyhow!("cannot publish trial state event: {error}"));
        }
        fs::remove_file(&temporary)?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}

fn validate_existing_chain(
    events: &[TrialStateFileV1],
    preregistration_sha256: &str,
) -> Result<()> {
    let mut last = "0".repeat(64);
    let mut per_trial = BTreeMap::new();
    for (sequence, file) in events.iter().enumerate() {
        if file.event.sequence != sequence as u64
            || file.event.previous_event_sha256 != last
            || file.event_sha256 != hash_event(&file.event)?
        {
            bail!("trial state journal hash chain is invalid");
        }
        validate_event(&file.event, preregistration_sha256)?;
        let prior = per_trial.insert(file.event.trial_id.clone(), file.event.phase);
        validate_transition(prior, file.event.phase)?;
        last = file.event_sha256.clone();
    }
    Ok(())
}

fn validate_transition(previous: Option<TrialPhaseV1>, next: TrialPhaseV1) -> Result<()> {
    let valid = match previous {
        None => next == TrialPhaseV1::FixtureReady,
        Some(TrialPhaseV1::FixtureReady) => {
            matches!(
                next,
                TrialPhaseV1::WorkspaceMaterialized | TrialPhaseV1::Failed
            )
        }
        Some(TrialPhaseV1::WorkspaceMaterialized) => matches!(
            next,
            TrialPhaseV1::ExecutionStarted | TrialPhaseV1::Failed | TrialPhaseV1::Interrupted
        ),
        Some(TrialPhaseV1::ExecutionStarted) => matches!(
            next,
            TrialPhaseV1::RecoveryExported | TrialPhaseV1::Failed | TrialPhaseV1::Interrupted
        ),
        Some(TrialPhaseV1::RecoveryExported) => matches!(
            next,
            TrialPhaseV1::RecoveryProven | TrialPhaseV1::Failed | TrialPhaseV1::Interrupted
        ),
        Some(TrialPhaseV1::RecoveryProven) => matches!(
            next,
            TrialPhaseV1::VerifierCompleted | TrialPhaseV1::Failed | TrialPhaseV1::Interrupted
        ),
        Some(TrialPhaseV1::VerifierCompleted) => {
            matches!(next, TrialPhaseV1::Settled | TrialPhaseV1::Failed)
        }
        Some(phase) if phase.terminal() => false,
        Some(_) => false,
    };
    if !valid {
        bail!("illegal durable trial state transition");
    }
    Ok(())
}

fn validate_event(event: &TrialStateEventV1, preregistration_sha256: &str) -> Result<()> {
    if event.schema_version != TRIAL_STATE_SCHEMA_V1
        || event.preregistration_sha256 != preregistration_sha256
        || validate_sha256(&event.previous_event_sha256).is_err()
        || event.slot_index >= 72
        || chrono::DateTime::parse_from_rfc3339(&event.occurred_at).is_err()
    {
        bail!("trial state event is invalid");
    }
    validate_trial_id(&event.trial_id)?;
    validate_optional_id("workspace_id", event.workspace_id.as_deref(), 128)?;
    validate_optional_sha256(event.recovery_artifact_sha256.as_deref())?;
    validate_optional_id("reason_code", event.reason_code.as_deref(), 128)?;
    Ok(())
}

fn read_owner_event(path: &Path) -> Result<TrialStateFileV1> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_EVENT_BYTES as u64
    {
        bail!("trial state event file is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("trial state event is not owner-only");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).context("trial state event is not strict JSON")
}

fn hash_event(event: &TrialStateEventV1) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(EVENT_HASH_DOMAIN);
    hash.update(serde_json::to_vec(event)?);
    Ok(encode_hex(&hash.finalize()))
}

fn validate_trial_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        bail!("trial ID is invalid");
    }
    Ok(())
}

fn validate_optional_id(label: &str, value: Option<&str>, max_bytes: usize) -> Result<()> {
    if let Some(value) = value {
        if value.is_empty()
            || value.len() > max_bytes
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            bail!("trial state {label} is invalid");
        }
    }
    Ok(())
}

fn validate_optional_sha256(value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_sha256(value)?;
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("SHA-256 value is invalid");
    }
    Ok(())
}

fn require_owner_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("trial state directory is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("trial state directory is not owner-only");
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn require_unix() -> Result<()> {
    #[cfg(not(unix))]
    bail!("RND-097 trial state requires Unix owner-only files");
    #[cfg(unix)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (tempfile::TempDir, TrialStateJournal) {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().canonicalize().unwrap().join("experiment");
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let journal = TrialStateJournal::create(&root, &"a".repeat(64)).unwrap();
        (outer, journal)
    }

    #[test]
    fn journal_requires_durable_order_and_refuses_ambiguous_resume() {
        let (_outer, journal) = journal();
        journal
            .append("trial-000", 0, TrialPhaseV1::FixtureReady, None, None, None)
            .unwrap();
        journal
            .append(
                "trial-000",
                0,
                TrialPhaseV1::WorkspaceMaterialized,
                Some("workspace-000"),
                None,
                None,
            )
            .unwrap();
        journal
            .append(
                "trial-000",
                0,
                TrialPhaseV1::ExecutionStarted,
                Some("workspace-000"),
                None,
                None,
            )
            .unwrap();
        journal
            .append(
                "trial-000",
                0,
                TrialPhaseV1::Interrupted,
                Some("workspace-000"),
                None,
                Some("worker_lost"),
            )
            .unwrap();
        assert!(journal
            .append(
                "trial-000",
                0,
                TrialPhaseV1::RecoveryExported,
                None,
                None,
                None
            )
            .is_err());
        assert_eq!(journal.events().unwrap().len(), 4);
    }

    #[test]
    fn journal_rejects_illegal_transition_and_hash_tampering() {
        let (outer, journal) = journal();
        assert!(journal
            .append(
                "trial-001",
                1,
                TrialPhaseV1::ExecutionStarted,
                None,
                None,
                None
            )
            .is_err());
        journal
            .append("trial-001", 1, TrialPhaseV1::FixtureReady, None, None, None)
            .unwrap();
        let file = outer
            .path()
            .canonicalize()
            .unwrap()
            .join("experiment")
            .join(JOURNAL_DIRECTORY)
            .join("00000000000000000000.json");
        fs::write(file, b"{}").unwrap();
        assert!(journal.events().is_err());
    }

    #[test]
    fn read_only_open_never_creates_a_journal() {
        let outer = tempfile::tempdir().unwrap();
        let root = outer.path().canonicalize().unwrap().join("experiment");
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

        assert!(!TrialStateJournal::exists(&root).unwrap());
        assert!(TrialStateJournal::open_existing(&root, &"a".repeat(64)).is_err());
        assert!(!root.join(JOURNAL_DIRECTORY).exists());

        let created = TrialStateJournal::create(&root, &"a".repeat(64)).unwrap();
        assert!(TrialStateJournal::exists(&root).unwrap());
        assert_eq!(
            TrialStateJournal::open_existing(&root, &"a".repeat(64))
                .unwrap()
                .events()
                .unwrap(),
            created.events().unwrap()
        );
    }
}
