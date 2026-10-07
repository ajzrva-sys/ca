use super::{finalize_record, verify_record, ToolEventV1, VaultRecord};
use anyhow::{bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;
const MIN_FREE_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const MIN_FREE_PERCENT: u64 = 5;

#[derive(Debug, Clone, Serialize, serde::Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VaultHealthV1 {
    pub schema: String,
    pub healthy: bool,
    pub root: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub minimum_free_bytes: u64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LearningVault {
    root: PathBuf,
}

impl LearningVault {
    pub fn machine() -> Result<Self> {
        let home = std::env::var_os("HOME").context("HOME is required for self-improvement")?;
        let home = PathBuf::from(home);
        let metadata = fs::symlink_metadata(&home).context("inspect HOME")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("HOME must be a real directory");
        }
        let home = fs::canonicalize(home)?;
        let codex_team = home.join(".codex-team");
        ensure_private_directory(&codex_team)?;
        Self::open_at(codex_team.join("self-improvement"))
    }

    /// Discover an enrolled vault without creating machine state. Legacy and
    /// disabled configurations therefore remain byte-for-byte inert.
    pub fn existing_machine() -> Result<Option<Self>> {
        let Some(home) = std::env::var_os("HOME") else {
            return Ok(None);
        };
        let home = PathBuf::from(home);
        let metadata = fs::symlink_metadata(&home).context("inspect HOME")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("HOME must be a real directory");
        }
        let root = fs::canonicalize(home)?
            .join(".codex-team")
            .join("self-improvement");
        Self::existing_at(root)
    }

    /// Open a vault for observation without creating or repairing any path.
    pub fn existing_at(root: PathBuf) -> Result<Option<Self>> {
        validate_absolute_path(&root)?;
        match fs::symlink_metadata(&root) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    bail!("self-improvement vault root must be a real directory");
                }
                inspect_private_directory(&root)?;
                Ok(Some(Self {
                    root: fs::canonicalize(root)?,
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn open_at(root: PathBuf) -> Result<Self> {
        validate_absolute_path(&root)?;
        ensure_private_directory(&root)?;
        let root = fs::canonicalize(&root)?;
        let vault = Self { root };
        for relative in [
            "harness",
            "harness/plans",
            "harness/cycles",
            "harness/receipts",
            "harness/attempts",
            "harness/generations",
            "harness/engineering",
            "harness/inputs",
            "harness/source",
            "harness/source-generations",
            "harness/source-checkouts",
            "harness/source-builds",
            "traces",
            "capsules",
            "objectives",
            "artifacts",
            "artifacts/content",
            "overlays",
            "evaluations/plans",
            "evaluations/receipts",
            "deployments/candidates",
            "deployments/receipts",
            "deployments/regressions",
            "supervisor",
            "supervisor/slots/ca/a",
            "supervisor/slots/ca/b",
            "supervisor/slots/codex-team/a",
            "supervisor/slots/codex-team/b",
            "locks",
        ] {
            vault.ensure_relative_directory(relative)?;
        }
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn health(&self) -> Result<VaultHealthV1> {
        let (free_bytes, total_bytes) = filesystem_space(&self.root)?;
        let percent_floor = total_bytes.saturating_mul(MIN_FREE_PERCENT) / 100;
        let minimum_free_bytes = MIN_FREE_BYTES.max(percent_floor);
        let mut issues = Vec::new();
        if free_bytes < minimum_free_bytes {
            issues.push(format!(
                "free space is below the learning floor ({free_bytes} < {minimum_free_bytes})"
            ));
        }
        if let Err(error) = inspect_private_directory(&self.root) {
            issues.push(error.to_string());
        }
        Ok(VaultHealthV1 {
            schema: "codex-team.self_improvement_vault_health.v1".to_string(),
            healthy: issues.is_empty(),
            root: self.root.display().to_string(),
            free_bytes,
            total_bytes,
            minimum_free_bytes,
            issues,
        })
    }

    pub fn acquire_lock(&self) -> Result<VaultLock> {
        self.acquire_named_lock("learner")
    }

    pub fn acquire_worker_lock(&self) -> Result<VaultLock> {
        self.acquire_named_lock("artifact-worker")
    }

    /// Own the whole harness driver, including the gaps between timed workers.
    pub fn acquire_harness_controller_lock(&self) -> Result<HarnessControllerLock> {
        Ok(HarnessControllerLock {
            _lock: self.acquire_named_lock("harness-controller")?,
            root: self.root.clone(),
        })
    }

    fn acquire_named_lock(&self, name: &str) -> Result<VaultLock> {
        let path = self.resolve_relative(&format!("locks/{name}.lock"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .with_context(|| format!("open global {name} lock {}", path.display()))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            bail!("another self-improvement process owns the {name} lock");
        }
        Ok(VaultLock { file })
    }

    pub fn write_immutable<T: VaultRecord + DeserializeOwned>(
        &self,
        record: &mut T,
    ) -> Result<PathBuf> {
        let health = self.health()?;
        if !health.healthy {
            bail!("learning vault is paused: {}", health.issues.join("; "));
        }
        finalize_record(record)?;
        let directory = record.record_directory();
        self.ensure_relative_directory(directory)?;
        let relative = format!("{directory}/{}.json", record.record_id());
        let path = self.resolve_relative(&relative)?;
        let bytes = serde_json::to_vec_pretty(record)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            bail!("learning record exceeds the 16 MiB bound");
        }
        if path.exists() {
            let existing: T = self.read_record_path(&path)?;
            verify_record(&existing)?;
            if serde_json::to_value(&existing)? == serde_json::to_value(record)? {
                return Ok(path);
            }
            bail!("immutable learning record already exists with different content");
        }
        write_private_atomic(&path, &bytes, false)?;
        Ok(path)
    }

    pub fn read_immutable<T: VaultRecord + DeserializeOwned>(
        &self,
        directory: &str,
        id: &str,
    ) -> Result<T> {
        super::validate_common_identifier(id, "record id")?;
        let path = self.resolve_relative(&format!("{directory}/{id}.json"))?;
        let record: T = self.read_record_path(&path)?;
        if record.record_directory() != directory || record.record_id() != id {
            bail!("learning record identity does not match its path");
        }
        verify_record(&record)?;
        Ok(record)
    }

    pub fn read_optional_immutable<T: VaultRecord + DeserializeOwned>(
        &self,
        directory: &str,
        id: &str,
    ) -> Result<Option<T>> {
        super::validate_common_identifier(id, "record id")?;
        let path = self.resolve_relative(&format!("{directory}/{id}.json"))?;
        match fs::symlink_metadata(&path) {
            Ok(_) => self.read_immutable(directory, id).map(Some),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn list_immutable<T: VaultRecord + DeserializeOwned>(
        &self,
        directory: &str,
    ) -> Result<Vec<T>> {
        let path = self.resolve_relative(directory)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        }
        inspect_private_directory(&path)?;
        let mut records = Vec::new();
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let metadata = fs::symlink_metadata(&entry_path)?;
            if metadata.file_type().is_symlink() {
                bail!("learning catalog contains a non-regular entry");
            }
            if metadata.is_dir() {
                continue;
            }
            if !metadata.is_file() {
                bail!("learning catalog contains a non-regular entry");
            }
            if entry_path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let record: T = self.read_record_path(&entry_path)?;
            verify_record(&record)?;
            records.push(record);
        }
        records.sort_by(|left, right| left.record_id().cmp(right.record_id()));
        Ok(records)
    }

    pub fn write_mutable<T: Serialize>(&self, relative: &str, value: &T) -> Result<PathBuf> {
        let path = self.resolve_relative(relative)?;
        if let Some(parent) = path.parent() {
            ensure_private_directory(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(value)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            bail!("mutable learning record exceeds the 16 MiB bound");
        }
        write_private_atomic(&path, &bytes, true)?;
        Ok(path)
    }

    pub fn read_mutable<T: DeserializeOwned>(&self, relative: &str) -> Result<Option<T>> {
        let path = self.resolve_relative(relative)?;
        if !path.exists() {
            return Ok(None);
        }
        self.read_record_path(&path).map(Some)
    }

    pub fn remove_mutable(&self, relative: &str) -> Result<bool> {
        let path = self.resolve_relative(relative)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    bail!("mutable learning record target is not a regular file");
                }
                fs::remove_file(&path)?;
                File::open(path.parent().context("mutable record has no parent")?)?.sync_all()?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn resolve_relative(&self, relative: &str) -> Result<PathBuf> {
        let relative = Path::new(relative);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("vault path must be safe and relative");
        }
        let path = self.root.join(relative);
        if !path.starts_with(&self.root) {
            bail!("vault path escapes the machine learning root");
        }
        Ok(path)
    }

    fn ensure_relative_directory(&self, relative: &str) -> Result<PathBuf> {
        let path = self.resolve_relative(relative)?;
        let mut current = self.root.clone();
        for component in Path::new(relative).components() {
            if let Component::Normal(name) = component {
                current.push(name);
                ensure_private_directory(&current)?;
            }
        }
        Ok(path)
    }

    fn read_record_path<T: DeserializeOwned>(&self, path: &Path) -> Result<T> {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("inspect learning record {}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_RECORD_BYTES
            || metadata.permissions().mode() & 0o077 != 0
        {
            bail!("learning record must be private, bounded, regular, and non-symlink");
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            bail!("learning record exceeds the 16 MiB bound");
        }
        serde_json::from_slice(&bytes).context("parse strict learning record")
    }
}

pub struct VaultLock {
    file: File,
}

pub struct HarnessControllerLock {
    _lock: VaultLock,
    root: PathBuf,
}

impl HarnessControllerLock {
    pub fn matches(&self, vault: &LearningVault) -> bool {
        self.root == vault.root
    }
}

impl Drop for VaultLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub fn sanitize_trace_text(input: &str) -> String {
    const MAX_CHARS: usize = 256 * 1024;
    let mut output = String::new();
    for line in input.lines() {
        let lower = line.to_ascii_lowercase();
        if secret_bearing_line(&lower) {
            output.push_str("[REDACTED SECRET-BEARING LINE]\n");
        } else {
            output.push_str(line);
            output.push('\n');
        }
        if output.chars().count() >= MAX_CHARS {
            output = output.chars().take(MAX_CHARS).collect();
            output.push_str("\n...[bounded]...\n");
            break;
        }
    }
    output
}

pub fn trace_text_contains_secret_pattern(input: &str) -> bool {
    input
        .lines()
        .any(|line| secret_bearing_line(&line.to_ascii_lowercase()))
}

pub fn sanitize_known_tool_event(
    name: &str,
    arguments: &Value,
    result: &str,
    success: bool,
) -> ToolEventV1 {
    let mut safe_arguments = BTreeMap::new();
    if let Value::Object(fields) = arguments {
        for (key, value) in fields {
            if !secret_key(key) {
                safe_arguments.insert(key.clone(), sanitize_json(value));
            }
        }
    }
    ToolEventV1::Known {
        name: bounded_identifier(name),
        safe_arguments,
        result_summary: sanitize_trace_text(result).chars().take(8_192).collect(),
        success,
    }
}

pub fn opaque_tool_event(
    name: &str,
    arguments: &Value,
    result: &str,
    success: bool,
) -> ToolEventV1 {
    ToolEventV1::Opaque {
        name: bounded_identifier(name),
        arguments_sha256: hash_bytes(
            serde_json::to_string(arguments)
                .unwrap_or_default()
                .as_bytes(),
        ),
        result_sha256: hash_bytes(result.as_bytes()),
        success,
    }
}

fn sanitize_json(value: &Value) -> Value {
    match value {
        Value::String(value) => Value::String(sanitize_trace_text(value)),
        Value::Array(values) => Value::Array(values.iter().take(128).map(sanitize_json).collect()),
        Value::Object(values) => {
            let mut safe = Map::new();
            for (key, value) in values.iter().take(128) {
                if !secret_key(key) {
                    safe.insert(key.clone(), sanitize_json(value));
                }
            }
            Value::Object(safe)
        }
        other => other.clone(),
    }
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "authorization",
        "credential",
        "cookie",
        "api_key",
        "apikey",
        "password",
        "private_key",
        "secret",
        "session_token",
        "access_token",
        "refresh_token",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn secret_bearing_line(lower: &str) -> bool {
    lower.contains("authorization:")
        || lower.contains("bearer ")
        || lower.contains("api_key=")
        || lower.contains("api-key:")
        || lower.contains("cookie:")
        || lower.contains("password=")
        || lower.contains("private_key")
        || lower.contains("secret=")
        || lower.contains("token=")
        || lower.contains("keychain")
}

fn bounded_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':'))
        .take(160)
        .collect()
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_absolute_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        bail!("learning vault root must be an absolute non-traversing path");
    }
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                || !metadata.is_dir()
                || metadata.permissions().mode() & 0o077 != 0
            {
                bail!("learning directory must be a real directory");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).with_context(|| format!("create {}", path.display()))?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error.into()),
    }
    inspect_private_directory(path)
}

fn inspect_private_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("learning directory must be private, real, and non-symlink");
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("learning record target must be a regular non-symlink file");
        }
        if !replace {
            bail!("immutable learning record already exists");
        }
    }
    let parent = path.parent().context("learning record has no parent")?;
    inspect_private_directory(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("record");
    let temp = parent.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn filesystem_space(path: &Path) -> Result<(u64, u64)> {
    let c_path = CString::new(path.as_os_str().as_bytes())?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).context("inspect learning vault free space");
    }
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize;
    Ok((
        (stats.f_bavail as u64).saturating_mul(block_size),
        (stats.f_blocks as u64).saturating_mul(block_size),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::self_improvement::{finalize_record, LearningCapsuleV1, CAPSULE_SCHEMA};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-self-improve-{name}-{nonce}"))
    }

    fn capsule() -> LearningCapsuleV1 {
        LearningCapsuleV1 {
            schema: CAPSULE_SCHEMA.to_string(),
            capsule_id: "capsule-1".to_string(),
            reusable_problem: "problem".to_string(),
            decision: "decision".to_string(),
            result: "result".to_string(),
            applicability: vec!["rust".to_string()],
            evidence_ids: vec!["e1".to_string()],
            confidence_basis_points: 9000,
            supersedes: None,
            created_at: "now".to_string(),
            digest: String::new(),
        }
    }

    #[test]
    fn vault_writes_private_immutable_records_and_rejects_tamper() -> Result<()> {
        let root = temp_root("immutable");
        fs::create_dir_all(root.parent().unwrap())?;
        let vault = LearningVault::open_at(root.clone())?;
        let mut record = capsule();
        let path = vault.write_immutable(&mut record)?;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            vault.read_immutable::<LearningCapsuleV1>("capsules", "capsule-1")?,
            record
        );

        let mut tampered = record.clone();
        tampered.result = "changed".to_string();
        vault.write_mutable("capsules/capsule-1.json", &tampered)?;
        assert!(vault
            .read_immutable::<LearningCapsuleV1>("capsules", "capsule-1")
            .is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn vault_rejects_symlink_root_and_traversal() -> Result<()> {
        let parent = temp_root("symlink");
        let actual = parent.join("actual");
        let link = parent.join("link");
        fs::create_dir_all(&actual)?;
        std::os::unix::fs::symlink(&actual, &link)?;
        assert!(LearningVault::open_at(link).is_err());
        let vault = LearningVault::open_at(parent.join("vault"))?;
        assert!(vault.resolve_relative("../escape").is_err());
        fs::remove_dir_all(parent)?;
        Ok(())
    }

    #[test]
    fn observation_never_creates_or_repairs_a_vault() -> Result<()> {
        let root = temp_root("read-only-open");
        assert!(LearningVault::existing_at(root.clone())?.is_none());
        assert!(!root.exists());

        fs::create_dir_all(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
        assert!(LearningVault::existing_at(root.clone()).is_err());
        assert_eq!(fs::metadata(&root)?.permissions().mode() & 0o777, 0o755);
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn trace_scrubber_removes_secret_fields_and_lines() {
        let event = sanitize_known_tool_event(
            "http",
            &serde_json::json!({
                "url": "https://example.invalid",
                "authorization": "Bearer canary-secret",
                "nested": {"password": "canary-password", "safe": "ok"}
            }),
            "authorization: Bearer response-canary\nordinary result",
            true,
        );
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("canary-secret"));
        assert!(!encoded.contains("canary-password"));
        assert!(!encoded.contains("response-canary"));
        assert!(encoded.contains("ordinary result"));
    }

    #[test]
    fn unknown_tool_arguments_are_hash_only() {
        let event = opaque_tool_event(
            "unknown",
            &serde_json::json!({"secret": "canary"}),
            "secret-result",
            false,
        );
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(!encoded.contains("canary"));
        assert!(!encoded.contains("secret-result"));
    }

    #[test]
    fn finalize_and_verify_detect_record_changes() -> Result<()> {
        let mut record = capsule();
        finalize_record(&mut record)?;
        verify_record(&record)?;
        record.result.push('!');
        assert!(verify_record(&record).is_err());
        Ok(())
    }
}
