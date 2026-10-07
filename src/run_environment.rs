use crate::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use crate::workspace::WorkspaceContext;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;
use tokio::io::AsyncReadExt;

const MAX_DIRENV_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_DIRENV_ERROR_BYTES: usize = 64 * 1024;
const MAX_ENVIRONMENT_KEYS: usize = 512;
const MAX_ENVIRONMENT_KEY_BYTES: usize = 128;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 16 * 1024;
const MAX_ENVRC_BYTES: u64 = 1024 * 1024;
const DIRENV_TIMEOUT: Duration = Duration::from_secs(30);

/// Process-scoped environment changes. Values are deliberately private and
/// this type has no serialization implementation so secrets cannot drift into
/// sessions, logs, or model-visible state.
#[derive(Clone, Default)]
pub struct RunEnvironment {
    changes: BTreeMap<String, Option<String>>,
    source_workspace: Option<PathBuf>,
    source_digest: Option<String>,
}

impl std::fmt::Debug for RunEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunEnvironment")
            .field("changed_keys", &self.changed_key_count())
            .field("removed_keys", &self.removed_key_count())
            .finish()
    }
}

impl RunEnvironment {
    pub fn ambient() -> Self {
        Self::default()
    }

    fn from_validated_changes(
        changes: BTreeMap<String, Option<String>>,
    ) -> Result<Self, RunEnvironmentError> {
        validate_changes(&changes)?;
        Ok(Self {
            changes,
            source_workspace: None,
            source_digest: None,
        })
    }

    #[cfg(test)]
    fn from_changes(changes: BTreeMap<String, Option<String>>) -> Self {
        Self {
            changes,
            source_workspace: None,
            source_digest: None,
        }
    }

    pub fn var(&self, name: &str) -> Option<String> {
        match self.changes.get(name) {
            Some(value) => value.clone(),
            None => std::env::var(name).ok(),
        }
    }

    pub fn var_os(&self, name: &str) -> Option<OsString> {
        match self.changes.get(name) {
            Some(Some(value)) => Some(OsString::from(value)),
            Some(None) => None,
            None => std::env::var_os(name),
        }
    }

    pub fn apply_tokio(&self, command: &mut tokio::process::Command) {
        for (name, value) in &self.changes {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
    }

    pub fn apply_std(&self, command: &mut std::process::Command) {
        for (name, value) in &self.changes {
            if let Some(value) = value {
                command.env(name, value);
            } else {
                command.env_remove(name);
            }
        }
    }

    pub fn changed_key_count(&self) -> usize {
        self.changes
            .values()
            .filter(|value| value.is_some())
            .count()
    }

    pub fn removed_key_count(&self) -> usize {
        self.changes
            .values()
            .filter(|value| value.is_none())
            .count()
    }

    pub fn merged(&self, delta: Self) -> Self {
        let switching_workspace = delta.source_workspace.is_some()
            && self.source_workspace.is_some()
            && delta.source_workspace != self.source_workspace;
        let mut changes = if switching_workspace {
            BTreeMap::new()
        } else {
            self.changes.clone()
        };
        changes.extend(delta.changes);
        Self {
            changes,
            source_workspace: delta
                .source_workspace
                .or_else(|| self.source_workspace.clone()),
            source_digest: delta.source_digest.or_else(|| self.source_digest.clone()),
        }
    }

    pub fn permits_workspace(&self, workspace: &WorkspaceContext) -> bool {
        self.source_workspace.as_ref().is_none_or(|source| {
            workspace
                .primary()
                .is_some_and(|primary| &primary.path == source)
        })
    }

    pub fn structured_snapshot(&self) -> EnvironmentSnapshotV1 {
        let mut added = Vec::new();
        let mut changed = Vec::new();
        let mut removed = Vec::new();
        for (name, value) in &self.changes {
            let safe_name = redact_environment_key(name);
            match (std::env::var(name).ok(), value) {
                (None, Some(_)) => added.push(safe_name),
                (Some(ambient), Some(value)) if ambient != *value => changed.push(safe_name),
                (Some(_), None) => removed.push(safe_name),
                _ => {}
            }
        }
        added.sort();
        added.dedup();
        changed.sort();
        changed.dedup();
        removed.sort();
        removed.dedup();
        EnvironmentSnapshotV1 {
            schema: "ca.environment-snapshot.v1",
            added,
            changed,
            removed,
            values_redacted: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnvironmentSnapshotV1 {
    pub schema: &'static str,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub values_redacted: bool,
}

fn redact_environment_key(name: &str) -> String {
    let folded = name.to_ascii_lowercase();
    if [
        "api_key",
        "apikey",
        "access_key",
        "secret",
        "token",
        "password",
        "passwd",
        "credential",
        "private_key",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|marker| folded.contains(marker))
    {
        "[redacted-secret-key]".into()
    } else {
        name.to_string()
    }
}

static PROCESS_ENVIRONMENT: OnceLock<RwLock<Arc<RunEnvironment>>> = OnceLock::new();

fn process_environment() -> &'static RwLock<Arc<RunEnvironment>> {
    PROCESS_ENVIRONMENT.get_or_init(|| RwLock::new(Arc::new(RunEnvironment::ambient())))
}

pub fn current() -> Arc<RunEnvironment> {
    process_environment()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub fn replace_process(environment: RunEnvironment) {
    *process_environment()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(environment);
}

pub fn var(name: &str) -> Option<String> {
    current().var(name)
}

pub fn var_os(name: &str) -> Option<OsString> {
    current().var_os(name)
}

pub fn apply_tokio(command: &mut tokio::process::Command) {
    current().apply_tokio(command);
}

pub fn apply_std(command: &mut std::process::Command) {
    current().apply_std(command);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirenvPreview {
    pub workspace: PathBuf,
    pub envrc: PathBuf,
    pub digest: String,
}

impl DirenvPreview {
    pub fn command_display(&self) -> &'static str {
        "direnv export json"
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RunEnvironmentError {
    #[error("workspace has no primary root")]
    NoPrimaryRoot,
    #[error("workspace does not contain a regular .envrc file")]
    MissingEnvrc,
    #[error("workspace .envrc must be a regular file and not a symlink")]
    UnsafeEnvrc,
    #[error("workspace .envrc exceeds the 1 MiB limit")]
    EnvrcTooLarge,
    #[error("workspace .envrc could not be read")]
    EnvrcRead,
    #[error("workspace .envrc changed after approval")]
    EnvrcDrift,
    #[error("direnv executable was not found in PATH")]
    DirenvUnavailable,
    #[error("direnv sandbox could not be prepared")]
    SandboxUnavailable,
    #[error("direnv could not be started")]
    StartFailed,
    #[error("direnv exceeded the 30 second execution limit")]
    Timeout,
    #[error("direnv export failed")]
    ExportFailed,
    #[error("direnv JSON output exceeds the 1 MiB limit")]
    OutputTooLarge,
    #[error("direnv did not return a JSON object containing string or null values")]
    MalformedOutput,
    #[error("direnv returned too many environment keys")]
    TooManyKeys,
    #[error("direnv returned an invalid environment key")]
    InvalidKey,
    #[error("direnv returned an oversized environment value")]
    ValueTooLarge,
}

pub fn inspect_direnv(workspace: &WorkspaceContext) -> Result<DirenvPreview, RunEnvironmentError> {
    let primary = workspace
        .primary()
        .ok_or(RunEnvironmentError::NoPrimaryRoot)?;
    let envrc = primary.path.join(".envrc");
    let metadata =
        std::fs::symlink_metadata(&envrc).map_err(|_| RunEnvironmentError::MissingEnvrc)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(RunEnvironmentError::UnsafeEnvrc);
    }
    if metadata.len() > MAX_ENVRC_BYTES {
        return Err(RunEnvironmentError::EnvrcTooLarge);
    }
    let bytes = std::fs::read(&envrc).map_err(|_| RunEnvironmentError::EnvrcRead)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok(DirenvPreview {
        workspace: primary.path.clone(),
        envrc,
        digest,
    })
}

pub async fn evaluate_direnv(
    workspace: &WorkspaceContext,
    approved: &DirenvPreview,
) -> Result<RunEnvironment, RunEnvironmentError> {
    let current_preview = inspect_direnv(workspace)?;
    if current_preview != *approved {
        return Err(RunEnvironmentError::EnvrcDrift);
    }
    let executable = resolve_executable("direnv").ok_or(RunEnvironmentError::DirenvUnavailable)?;
    let temporary = tempfile::tempdir().map_err(|_| RunEnvironmentError::StartFailed)?;
    let roots = workspace
        .roots()
        .iter()
        .map(|root| root.path.clone())
        .collect::<Vec<_>>();
    let prepared = SandboxBackend::new(SandboxMode::Required)
        .prepare_process(
            &executable,
            &["export".into(), "json".into()],
            &SandboxPolicy {
                roots,
                read_only_roots: Vec::new(),
                temporary_directory: temporary.path().to_path_buf(),
                network_allowed: false,
                hide_other_processes: true,
            },
        )
        .map_err(|_| RunEnvironmentError::SandboxUnavailable)?;
    let mut command = tokio::process::Command::new(prepared.program);
    command
        .args(prepared.arguments)
        .current_dir(&approved.workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    current().apply_tokio(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|_| RunEnvironmentError::StartFailed)?;
    let _process_group = DirenvProcessGroupGuard::new(child.id());
    let stdout = child
        .stdout
        .take()
        .ok_or(RunEnvironmentError::StartFailed)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(RunEnvironmentError::StartFailed)?;
    let outcome = tokio::time::timeout(DIRENV_TIMEOUT, async move {
        let stdout = read_bounded(stdout, MAX_DIRENV_OUTPUT_BYTES);
        let stderr = read_bounded(stderr, MAX_DIRENV_ERROR_BYTES);
        let (stdout, stderr, status) = tokio::join!(stdout, stderr, child.wait());
        (stdout, stderr, status)
    })
    .await
    .map_err(|_| RunEnvironmentError::Timeout)?;
    let stdout = outcome.0?;
    let _sanitized_stderr = outcome.1?;
    let status = outcome.2.map_err(|_| RunEnvironmentError::ExportFailed)?;
    if !status.success() {
        return Err(RunEnvironmentError::ExportFailed);
    }
    let mut environment = parse_direnv_export(&stdout)?;
    environment.source_workspace = Some(approved.workspace.clone());
    environment.source_digest = Some(approved.digest.clone());
    Ok(environment)
}

async fn read_bounded<R>(reader: R, limit: usize) -> Result<Vec<u8>, RunEnvironmentError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| RunEnvironmentError::ExportFailed)?;
    if bytes.len() > limit {
        return Err(RunEnvironmentError::OutputTooLarge);
    }
    Ok(bytes)
}

fn parse_direnv_export(bytes: &[u8]) -> Result<RunEnvironment, RunEnvironmentError> {
    if bytes.len() > MAX_DIRENV_OUTPUT_BYTES {
        return Err(RunEnvironmentError::OutputTooLarge);
    }
    let exported = serde_json::from_slice::<BTreeMap<String, serde_json::Value>>(bytes)
        .map_err(|_| RunEnvironmentError::MalformedOutput)?;
    if exported.len() > MAX_ENVIRONMENT_KEYS {
        return Err(RunEnvironmentError::TooManyKeys);
    }
    let mut changes = BTreeMap::new();
    for (name, value) in exported {
        validate_name(&name)?;
        let value = match value {
            serde_json::Value::String(value) => Some(value),
            serde_json::Value::Null => None,
            _ => return Err(RunEnvironmentError::MalformedOutput),
        };
        changes.insert(name, value);
    }
    RunEnvironment::from_validated_changes(changes)
}

fn validate_changes(changes: &BTreeMap<String, Option<String>>) -> Result<(), RunEnvironmentError> {
    if changes.len() > MAX_ENVIRONMENT_KEYS {
        return Err(RunEnvironmentError::TooManyKeys);
    }
    for (name, value) in changes {
        validate_name(name)?;
        if let Some(value) = value {
            if value.len() > MAX_ENVIRONMENT_VALUE_BYTES || value.as_bytes().contains(&0) {
                return Err(RunEnvironmentError::ValueTooLarge);
            }
        }
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), RunEnvironmentError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_ENVIRONMENT_KEY_BYTES
        && (name.as_bytes()[0].is_ascii_alphabetic() || name.as_bytes()[0] == b'_');
    let valid = valid
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(RunEnvironmentError::InvalidKey)
    }
}

fn resolve_executable(name: &str) -> Option<PathBuf> {
    let path = var_os("PATH")?;
    std::env::split_paths(&path).find_map(|directory| {
        let candidate = directory.join(name);
        let metadata = std::fs::metadata(&candidate).ok()?;
        if metadata.file_type().is_file() {
            candidate.canonicalize().ok()
        } else {
            None
        }
    })
}

struct DirenvProcessGroupGuard {
    process_id: Option<u32>,
}

impl DirenvProcessGroupGuard {
    fn new(process_id: Option<u32>) -> Self {
        Self { process_id }
    }
}

impl Drop for DirenvProcessGroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(process_id) = self.process_id.and_then(|id| i32::try_from(id).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(process_id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn debug_never_exposes_values() {
        let environment = RunEnvironment::from_changes(BTreeMap::from([
            ("CA_ENV_SECRET".into(), Some("do-not-print".into())),
            ("CA_ENV_REMOVED".into(), None),
        ]));
        let debug = format!("{environment:?}");
        assert!(!debug.contains("do-not-print"));
        assert!(!debug.contains("CA_ENV_SECRET"));
        assert!(debug.contains("changed_keys: 1"));
        assert!(debug.contains("removed_keys: 1"));
    }

    #[test]
    fn structured_snapshot_is_key_only_and_redacts_secret_shaped_names() {
        let environment = RunEnvironment::from_changes(BTreeMap::from([
            (
                "CA_PRINTENV_SENTINEL_TOKEN".into(),
                Some("never-serialize-this-secret".into()),
            ),
            (
                "CA_PRINTENV_SAFE_NAME".into(),
                Some("ordinary-value".into()),
            ),
        ]));
        let snapshot = environment.structured_snapshot();
        let rendered = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(snapshot.schema, "ca.environment-snapshot.v1");
        assert!(rendered.contains("[redacted-secret-key]"));
        assert!(rendered.contains("CA_PRINTENV_SAFE_NAME"));
        assert!(!rendered.contains("CA_PRINTENV_SENTINEL_TOKEN"));
        assert!(!rendered.contains("never-serialize-this-secret"));
        assert!(!rendered.contains("ordinary-value"));
    }

    #[test]
    fn applies_added_and_removed_values_without_serialization() {
        let environment = RunEnvironment::from_changes(BTreeMap::from([
            ("CA_ENV_ADDED".into(), Some("present".into())),
            ("CA_ENV_REMOVED".into(), None),
        ]));
        let mut command = std::process::Command::new("/usr/bin/env");
        command.env("CA_ENV_REMOVED", "ambient");
        environment.apply_std(&mut command);
        let output = command.output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.lines().any(|line| line == "CA_ENV_ADDED=present"));
        assert!(!stdout
            .lines()
            .any(|line| line.starts_with("CA_ENV_REMOVED=")));
    }

    #[test]
    fn re_evaluation_delta_preserves_prior_overlay_without_exposing_values() {
        let first = RunEnvironment::from_changes(BTreeMap::from([(
            "CA_ENV_FIRST".into(),
            Some("first-secret".into()),
        )]));
        let second = RunEnvironment::from_changes(BTreeMap::from([
            ("CA_ENV_SECOND".into(), Some("second-secret".into())),
            ("CA_ENV_FIRST".into(), None),
        ]));
        let merged = first.merged(second);
        assert_eq!(
            merged.var("CA_ENV_SECOND").as_deref(),
            Some("second-secret")
        );
        assert_eq!(merged.var("CA_ENV_FIRST"), None);
        let debug = format!("{merged:?}");
        assert!(!debug.contains("first-secret"));
        assert!(!debug.contains("second-secret"));
    }

    #[test]
    fn bound_overlay_refuses_a_different_primary_workspace() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_workspace = WorkspaceContext::discover(first.path()).unwrap();
        let second_workspace = WorkspaceContext::discover(second.path()).unwrap();
        let mut environment = RunEnvironment::from_changes(BTreeMap::new());
        environment.source_workspace = first_workspace
            .primary()
            .map(|primary| primary.path.clone());
        assert!(environment.permits_workspace(&first_workspace));
        assert!(!environment.permits_workspace(&second_workspace));
        assert!(!format!("{environment:?}").contains(&first.path().display().to_string()));
    }

    #[test]
    fn parser_rejects_non_string_invalid_and_oversized_exports() {
        assert!(matches!(
            parse_direnv_export(br#"{"GOOD":1}"#),
            Err(RunEnvironmentError::MalformedOutput)
        ));
        assert!(matches!(
            parse_direnv_export(br#"{"BAD-NAME":"x"}"#),
            Err(RunEnvironmentError::InvalidKey)
        ));
        let oversized = format!(
            r#"{{"GOOD":"{}"}}"#,
            "x".repeat(MAX_ENVIRONMENT_VALUE_BYTES + 1)
        );
        assert!(matches!(
            parse_direnv_export(oversized.as_bytes()),
            Err(RunEnvironmentError::ValueTooLarge)
        ));
        let removed = parse_direnv_export(br#"{"REMOVED":null}"#).unwrap();
        assert_eq!(removed.removed_key_count(), 1);
    }

    #[test]
    fn inspect_requires_regular_bounded_envrc_and_detects_digest_drift() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".envrc"), "export A=1\n").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let first = inspect_direnv(&workspace).unwrap();
        assert_eq!(first.workspace, root.path().canonicalize().unwrap());
        std::fs::write(root.path().join(".envrc"), "export A=2\n").unwrap();
        let second = inspect_direnv(&workspace).unwrap();
        assert_ne!(first.digest, second.digest);
    }

    #[tokio::test]
    async fn evaluation_rejects_digest_drift_before_executable_lookup() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".envrc"), "export A=1\n").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let approved = inspect_direnv(&workspace).unwrap();
        std::fs::write(root.path().join(".envrc"), "export A=2\n").unwrap();
        assert!(matches!(
            evaluate_direnv(&workspace, &approved).await,
            Err(RunEnvironmentError::EnvrcDrift)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn inspect_rejects_symlinked_envrc() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, "export SECRET=outside\n").unwrap();
        symlink(&outside, root.path().join(".envrc")).unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        assert!(matches!(
            inspect_direnv(&workspace),
            Err(RunEnvironmentError::UnsafeEnvrc)
        ));
    }
}
