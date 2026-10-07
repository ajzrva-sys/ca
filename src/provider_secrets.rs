use anyhow::Context;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "ca.provider-api-key";
const CREDENTIAL_REFERENCE_PREFIX: &str = "credential://ca/provider/";
const PROFILE_CREDENTIAL_REFERENCE_PREFIX: &str = "credential://ca/profile/";
const KEYCHAIN_REFERENCE_PREFIX: &str = "keychain://ca/provider/";
const PROFILE_KEYCHAIN_REFERENCE_PREFIX: &str = "keychain://ca/profile/";
const MAX_SECRET_BYTES: u64 = 64 * 1024;

pub trait ProviderSecretStore: Send + Sync {
    fn load(&self, provider: &str) -> anyhow::Result<Option<Vec<u8>>>;
    fn store(&self, provider: &str, secret: &[u8]) -> anyhow::Result<()>;
    fn delete(&self, provider: &str) -> anyhow::Result<()>;
}

pub fn system_provider_secret_store(
    profile: &crate::runtime::ActiveProfile,
) -> Arc<dyn ProviderSecretStore> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Arc::new(MemoryProviderSecretStore::default());
    }
    let root = crate::runtime::RuntimePaths::discover()
        .ok()
        .map(|paths| paths.profile_data_dir(profile).join("credentials"));
    let primary: Arc<dyn ProviderSecretStore> = Arc::new(SystemProviderSecretStore { root });
    #[cfg(target_os = "macos")]
    {
        Arc::new(MigratingProviderSecretStore::new(
            primary,
            Arc::new(LegacyKeychainProviderSecretStore),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    {
        primary
    }
}

pub fn credential_reference(provider: &str) -> String {
    format!("{CREDENTIAL_REFERENCE_PREFIX}{provider}")
}

pub fn profile_credential_reference(profile: &str, provider: &str) -> String {
    format!("{PROFILE_CREDENTIAL_REFERENCE_PREFIX}{profile}/provider/{provider}")
}

pub fn profile_credential_reference_parts(value: &str) -> Option<(&str, &str)> {
    profile_reference_parts(value, PROFILE_CREDENTIAL_REFERENCE_PREFIX)
}

pub fn credential_reference_provider(value: &str) -> Option<&str> {
    reference_provider(value, CREDENTIAL_REFERENCE_PREFIX)
}

#[cfg(test)]
pub fn keychain_reference(provider: &str) -> String {
    format!("{KEYCHAIN_REFERENCE_PREFIX}{provider}")
}

#[cfg(test)]
pub fn profile_keychain_reference(profile: &str, provider: &str) -> String {
    format!("{PROFILE_KEYCHAIN_REFERENCE_PREFIX}{profile}/provider/{provider}")
}

pub fn profile_keychain_reference_parts(value: &str) -> Option<(&str, &str)> {
    profile_reference_parts(value, PROFILE_KEYCHAIN_REFERENCE_PREFIX)
}

fn profile_reference_parts<'a>(value: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let remainder = value.strip_prefix(prefix)?;
    let (profile, provider) = remainder.split_once("/provider/")?;
    (valid_profile_reference_component(profile) && valid_reference_component(provider))
        .then_some((profile, provider))
}

pub fn keychain_reference_provider(value: &str) -> Option<&str> {
    reference_provider(value, KEYCHAIN_REFERENCE_PREFIX)
}

fn reference_provider<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let provider = value.strip_prefix(prefix)?;
    valid_reference_component(provider).then_some(provider)
}

pub fn resembles_saved_reference(value: &str) -> bool {
    value.starts_with(CREDENTIAL_REFERENCE_PREFIX)
        || value.starts_with(PROFILE_CREDENTIAL_REFERENCE_PREFIX)
        || value.starts_with(KEYCHAIN_REFERENCE_PREFIX)
        || value.starts_with(PROFILE_KEYCHAIN_REFERENCE_PREFIX)
}

#[cfg(test)]
pub fn resembles_keychain_reference(value: &str) -> bool {
    value.starts_with(KEYCHAIN_REFERENCE_PREFIX)
        || value.starts_with(PROFILE_KEYCHAIN_REFERENCE_PREFIX)
}

fn valid_reference_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

fn valid_profile_reference_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

#[derive(Default)]
pub struct SecretValue {
    bytes: Vec<u8>,
}

impl SecretValue {
    pub fn from_text(value: &str) -> Self {
        Self {
            bytes: value.as_bytes().to_vec(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn clear(&mut self) {
        self.bytes.fill(0);
        self.bytes.clear();
    }

    pub fn insert_char(&mut self, cursor: usize, character: char) -> usize {
        let cursor = self.floor_char_boundary(cursor);
        let mut encoded = [0_u8; 4];
        let encoded = character.encode_utf8(&mut encoded).as_bytes();
        self.bytes.splice(cursor..cursor, encoded.iter().copied());
        cursor + encoded.len()
    }

    pub fn previous_char_boundary(&self, cursor: usize) -> usize {
        let value = self.as_str();
        let cursor = floor_char_boundary(value, cursor);
        value[..cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    pub fn next_char_boundary(&self, cursor: usize) -> usize {
        let value = self.as_str();
        let cursor = floor_char_boundary(value, cursor);
        value[cursor..]
            .char_indices()
            .nth(1)
            .map(|(offset, _)| cursor + offset)
            .unwrap_or(value.len())
    }

    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        let start = self.floor_char_boundary(range.start);
        let end = self.floor_char_boundary(range.end).max(start);
        self.bytes[start..end].fill(0);
        self.bytes.drain(start..end);
    }

    pub fn with_secret<R>(&self, consumer: impl FnOnce(&[u8]) -> R) -> R {
        consumer(&self.bytes)
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).expect("secret input is assembled from UTF-8 characters")
    }

    pub fn floor_char_boundary(&self, cursor: usize) -> usize {
        floor_char_boundary(self.as_str(), cursor)
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.bytes.fill(0);
    }
}

fn floor_char_boundary(value: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(value.len());
    while !value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[derive(Debug, Default)]
struct MemoryProviderSecretStore {
    values: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl ProviderSecretStore for MemoryProviderSecretStore {
    fn load(&self, provider: &str) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self
            .values
            .lock()
            .map_err(|_| anyhow::anyhow!("in-memory credential store is unavailable"))?
            .get(provider)
            .cloned())
    }

    fn store(&self, provider: &str, secret: &[u8]) -> anyhow::Result<()> {
        if secret.len() as u64 > MAX_SECRET_BYTES {
            anyhow::bail!("credential is too large");
        }
        self.values
            .lock()
            .map_err(|_| anyhow::anyhow!("in-memory credential store is unavailable"))?
            .insert(provider.to_string(), secret.to_vec());
        Ok(())
    }

    fn delete(&self, provider: &str) -> anyhow::Result<()> {
        if let Some(mut secret) = self
            .values
            .lock()
            .map_err(|_| anyhow::anyhow!("in-memory credential store is unavailable"))?
            .remove(provider)
        {
            secret.fill(0);
        }
        Ok(())
    }
}

struct MigratingProviderSecretStore {
    primary: Arc<dyn ProviderSecretStore>,
    legacy: Arc<dyn ProviderSecretStore>,
}

impl MigratingProviderSecretStore {
    fn new(primary: Arc<dyn ProviderSecretStore>, legacy: Arc<dyn ProviderSecretStore>) -> Self {
        Self { primary, legacy }
    }
}

impl std::fmt::Debug for MigratingProviderSecretStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MigratingProviderSecretStore")
            .field("primary", &"[PRIVATE PROVIDER CREDENTIAL STORE]")
            .field("legacy", &"[LEGACY KEYCHAIN STORE]")
            .finish()
    }
}

impl ProviderSecretStore for MigratingProviderSecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        if let Some(secret) = self.primary.load(account)? {
            return Ok(Some(secret));
        }
        let Some(mut secret) = self.legacy.load(account).context(
            "macOS Keychain could not read the legacy provider credential for one-time migration",
        )?
        else {
            return Ok(None);
        };
        if secret.is_empty() {
            anyhow::bail!("legacy provider credential is empty");
        }
        if secret.len() as u64 > MAX_SECRET_BYTES {
            secret.fill(0);
            anyhow::bail!("legacy provider credential is too large");
        }
        if let Err(error) = self.primary.store(account, &secret) {
            secret.fill(0);
            return Err(error).context(
                "could not migrate the legacy provider credential into the private provider credential store",
            );
        }
        Ok(Some(secret))
    }

    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        self.primary.store(account, secret)
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        self.primary.delete(account)
    }
}

struct SystemProviderSecretStore {
    root: Option<PathBuf>,
}

#[cfg(target_os = "macos")]
struct LegacyKeychainProviderSecretStore;

#[cfg(target_os = "macos")]
impl ProviderSecretStore for LegacyKeychainProviderSecretStore {
    fn load(&self, provider: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let options = security_framework::passwords::PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            provider,
        );
        match security_framework::passwords::generic_password(options) {
            Ok(mut secret) if secret.len() as u64 > MAX_SECRET_BYTES => {
                secret.fill(0);
                anyhow::bail!("saved credential is too large")
            }
            Ok(secret) => Ok(Some(secret)),
            Err(error) if error.code() == -25_300 => Ok(None),
            Err(error) => Err(error).context("macOS Keychain could not read the API key"),
        }
    }

    fn store(&self, provider: &str, secret: &[u8]) -> anyhow::Result<()> {
        if secret.is_empty() {
            anyhow::bail!("credential is empty");
        }
        if secret.len() as u64 > MAX_SECRET_BYTES {
            anyhow::bail!("credential is too large");
        }
        security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, provider, secret)
            .context("macOS Keychain could not save the API key")
    }

    fn delete(&self, provider: &str) -> anyhow::Result<()> {
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, provider) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25_300 => Ok(()),
            Err(error) => Err(error).context("macOS Keychain could not delete the API key"),
        }
    }
}

impl ProviderSecretStore for SystemProviderSecretStore {
    fn load(&self, provider: &str) -> anyhow::Result<Option<Vec<u8>>> {
        FileProviderSecretStore::new(
            self.root.clone().ok_or_else(|| {
                anyhow::anyhow!("HOME is not set; credential path is unavailable")
            })?,
        )
        .load(portable_account(provider))
    }

    fn store(&self, provider: &str, secret: &[u8]) -> anyhow::Result<()> {
        FileProviderSecretStore::new(
            self.root.clone().ok_or_else(|| {
                anyhow::anyhow!("HOME is not set; credential path is unavailable")
            })?,
        )
        .store(portable_account(provider), secret)
    }

    fn delete(&self, provider: &str) -> anyhow::Result<()> {
        FileProviderSecretStore::new(
            self.root.clone().ok_or_else(|| {
                anyhow::anyhow!("HOME is not set; credential path is unavailable")
            })?,
        )
        .delete(portable_account(provider))
    }
}

fn portable_account(account: &str) -> &str {
    account
        .strip_prefix("profile/")
        .and_then(|scoped| scoped.split_once("/provider/"))
        .map_or(account, |(_, provider)| provider)
}

#[derive(Debug, Clone)]
pub struct FileProviderSecretStore {
    root: PathBuf,
}

impl FileProviderSecretStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn credential_path(&self, account: &str) -> anyhow::Result<PathBuf> {
        let parts = account.split('/').collect::<Vec<_>>();
        let valid = match parts.as_slice() {
            [provider] => valid_reference_component(provider),
            ["profile", profile, "provider", provider] => {
                valid_profile_reference_component(profile) && valid_reference_component(provider)
            }
            _ => false,
        };
        if !valid {
            anyhow::bail!("credential account is invalid");
        }
        let mut path = self.root.clone();
        for part in parts {
            path.push(part);
        }
        Ok(path)
    }

    fn directory_chain(&self, parent: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let relative = parent
            .strip_prefix(&self.root)
            .map_err(|_| anyhow::anyhow!("credential path escaped its store"))?;
        let mut chain = vec![self.root.clone()];
        let mut current = self.root.clone();
        for component in relative.components() {
            let std::path::Component::Normal(component) = component else {
                anyhow::bail!("credential directory is invalid");
            };
            current.push(component);
            chain.push(current.clone());
        }
        Ok(chain)
    }

    fn ensure_private_parent(&self, parent: &Path) -> anyhow::Result<()> {
        for directory in self.directory_chain(parent)? {
            Self::ensure_private_directory(&directory)?;
        }
        Ok(())
    }

    fn validate_private_parent(&self, parent: &Path) -> anyhow::Result<bool> {
        for directory in self.directory_chain(parent)? {
            let metadata = match std::fs::symlink_metadata(&directory) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("could not inspect {}", directory.display()))
                }
            };
            Self::validate_private_directory(&directory, &metadata)?;
        }
        Ok(true)
    }

    #[cfg(unix)]
    fn validate_private_directory(path: &Path, metadata: &std::fs::Metadata) -> anyhow::Result<()> {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            anyhow::bail!("credential directory is not a current-user directory");
        }
        if metadata.permissions().mode() & 0o077 != 0 {
            anyhow::bail!("credential directory must have mode 0700 or stricter");
        }
        let _ = path;
        Ok(())
    }

    #[cfg(not(unix))]
    fn validate_private_directory(
        _path: &Path,
        _metadata: &std::fs::Metadata,
    ) -> anyhow::Result<()> {
        anyhow::bail!("portable credential files require a Unix host")
    }

    #[cfg(unix)]
    fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
        use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
        if path.exists() {
            let metadata = std::fs::symlink_metadata(path)
                .with_context(|| format!("could not inspect {}", path.display()))?;
            if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
                anyhow::bail!("credential directory is not a current-user directory");
            }
            if metadata.mode() & 0o077 != 0 {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                    .context("could not secure credential directory")?;
            }
            return Ok(());
        }
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder
            .create(path)
            .with_context(|| format!("could not create {}", path.display()))?;
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("could not inspect {}", path.display()))?;
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            anyhow::bail!("credential directory is not a current-user directory");
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .context("could not secure credential directory")
    }

    #[cfg(not(unix))]
    fn ensure_private_directory(_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!("portable credential files require a Unix host")
    }

    #[cfg(unix)]
    fn validate_file(path: &Path, metadata: &std::fs::Metadata) -> anyhow::Result<()> {
        use std::os::unix::fs::MetadataExt as _;
        if !metadata.file_type().is_file() {
            anyhow::bail!("credential is not a regular file");
        }
        if metadata.uid() != unsafe { libc::geteuid() } {
            anyhow::bail!("credential is not owned by the current user");
        }
        if metadata.mode() & 0o077 != 0 {
            anyhow::bail!("credential must have mode 0600 or stricter");
        }
        if metadata.len() > MAX_SECRET_BYTES {
            anyhow::bail!("credential is too large");
        }
        let _ = path;
        Ok(())
    }

    #[cfg(not(unix))]
    fn validate_file(_path: &Path, _metadata: &std::fs::Metadata) -> anyhow::Result<()> {
        anyhow::bail!("portable credential files require a Unix host")
    }

    fn validate_existing_target(path: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => Self::validate_file(path, &metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("could not inspect credential"),
        }
    }
}

impl ProviderSecretStore for FileProviderSecretStore {
    fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
        use std::io::Read as _;
        let path = self.credential_path(account)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("credential path has no parent"))?;
        if !self.validate_private_parent(parent)? {
            return Ok(None);
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("could not open credential"),
        };
        let metadata = file.metadata().context("could not inspect credential")?;
        Self::validate_file(&path, &metadata)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_SECRET_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("could not read credential")?;
        if bytes.len() as u64 > MAX_SECRET_BYTES {
            bytes.fill(0);
            anyhow::bail!("credential is too large");
        }
        Ok(Some(bytes))
    }

    fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
        use std::io::Write as _;
        if secret.is_empty() {
            anyhow::bail!("credential is empty");
        }
        if secret.len() as u64 > MAX_SECRET_BYTES {
            anyhow::bail!("credential is too large");
        }
        let path = self.credential_path(account)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("credential path has no parent"))?;
        self.ensure_private_parent(parent)?;
        Self::validate_existing_target(&path)?;
        let temporary = parent.join(format!(".credential.{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        }
        let result = (|| -> anyhow::Result<()> {
            let mut file = options
                .open(&temporary)
                .context("could not create credential")?;
            file.write_all(secret)
                .context("could not write credential")?;
            file.sync_all().context("could not sync credential")?;
            std::fs::rename(&temporary, &path).context("could not replace credential")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn delete(&self, account: &str) -> anyhow::Result<()> {
        let path = self.credential_path(account)?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("credential path has no parent"))?;
        if !self.validate_private_parent(parent)? {
            return Ok(());
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                Self::validate_file(&path, &metadata)?;
                std::fs::remove_file(path).context("could not delete credential")
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("could not inspect credential"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct CountingStore {
        values: std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
        loads: std::sync::atomic::AtomicUsize,
        stores: std::sync::atomic::AtomicUsize,
        deletes: std::sync::atomic::AtomicUsize,
        fail_load: std::sync::atomic::AtomicBool,
        fail_store: std::sync::atomic::AtomicBool,
    }

    impl ProviderSecretStore for CountingStore {
        fn load(&self, account: &str) -> anyhow::Result<Option<Vec<u8>>> {
            self.loads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.fail_load.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("legacy credential access denied");
            }
            Ok(self.values.lock().unwrap().get(account).cloned())
        }

        fn store(&self, account: &str, secret: &[u8]) -> anyhow::Result<()> {
            self.stores
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if self.fail_store.load(std::sync::atomic::Ordering::Relaxed) {
                anyhow::bail!("primary migration write failed");
            }
            self.values
                .lock()
                .unwrap()
                .insert(account.to_string(), secret.to_vec());
            Ok(())
        }

        fn delete(&self, account: &str) -> anyhow::Result<()> {
            self.deletes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.values.lock().unwrap().remove(account);
            Ok(())
        }
    }

    #[test]
    fn secret_value_is_redacted_and_edits_on_unicode_boundaries() {
        let mut secret = SecretValue::default();
        let mut cursor = secret.insert_char(0, 's');
        cursor = secret.insert_char(cursor, 'ë');
        cursor = secret.insert_char(cursor, 'k');
        assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
        assert_eq!(cursor, "sëk".len());

        let previous = secret.previous_char_boundary(cursor);
        secret.remove(previous..cursor);
        assert_eq!(secret.with_secret(|bytes| bytes.to_vec()), "së".as_bytes());
        secret.clear();
        assert!(secret.is_empty());
    }

    #[test]
    fn migrating_store_imports_legacy_once_then_uses_only_primary() {
        let primary = Arc::new(CountingStore::default());
        let legacy = Arc::new(CountingStore::default());
        legacy
            .values
            .lock()
            .unwrap()
            .insert("mimo".into(), b"legacy-secret".to_vec());
        let store = MigratingProviderSecretStore::new(primary.clone(), legacy.clone());

        assert_eq!(store.load("mimo").unwrap().unwrap(), b"legacy-secret");
        assert_eq!(legacy.loads.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(primary.stores.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(store.load("mimo").unwrap().unwrap(), b"legacy-secret");
        assert_eq!(legacy.loads.load(std::sync::atomic::Ordering::Relaxed), 1);

        store.store("mimo", b"new-private-secret").unwrap();
        assert_eq!(store.load("mimo").unwrap().unwrap(), b"new-private-secret");
        assert_eq!(legacy.loads.load(std::sync::atomic::Ordering::Relaxed), 1);
        store.delete("mimo").unwrap();
        assert_eq!(
            primary.deletes.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(legacy.deletes.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert!(!format!("{store:?}").contains("legacy-secret"));
    }

    #[test]
    fn migrating_store_fails_safely_when_private_import_cannot_commit() {
        let primary = Arc::new(CountingStore::default());
        primary
            .fail_store
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let legacy = Arc::new(CountingStore::default());
        legacy
            .values
            .lock()
            .unwrap()
            .insert("mimo".into(), b"must-not-leak".to_vec());
        let store = MigratingProviderSecretStore::new(primary.clone(), legacy);

        let error = store.load("mimo").unwrap_err().to_string();
        assert!(error.contains("private provider credential"));
        assert!(!error.contains("must-not-leak"));
        assert!(!primary.values.lock().unwrap().contains_key("mimo"));
    }

    #[test]
    fn migrating_store_reports_legacy_denial_without_touching_primary() {
        let primary = Arc::new(CountingStore::default());
        let legacy = Arc::new(CountingStore::default());
        legacy
            .fail_load
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let store = MigratingProviderSecretStore::new(primary.clone(), legacy);

        let error = store.load("mimo").unwrap_err().to_string();
        assert!(error.contains("one-time migration"));
        assert!(!error.contains("mimo"));
        assert_eq!(primary.loads.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(primary.stores.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[cfg(unix)]
    #[test]
    fn migrating_store_commits_to_private_profile_file_before_reuse() {
        use std::os::unix::fs::PermissionsExt as _;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("profiles/work/credentials");
        let primary = Arc::new(SystemProviderSecretStore {
            root: Some(root.clone()),
        });
        let legacy = Arc::new(CountingStore::default());
        legacy.values.lock().unwrap().insert(
            "profile/work/provider/mimo".into(),
            b"legacy-profile-secret".to_vec(),
        );
        let store = MigratingProviderSecretStore::new(primary, legacy.clone());

        assert_eq!(
            store.load("profile/work/provider/mimo").unwrap().unwrap(),
            b"legacy-profile-secret"
        );
        let credential = root.join("mimo");
        assert_eq!(
            std::fs::metadata(&credential).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            store.load("profile/work/provider/mimo").unwrap().unwrap(),
            b"legacy-profile-secret"
        );
        assert_eq!(legacy.loads.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn profile_scoped_system_accounts_use_the_profile_credential_root() {
        assert_eq!(portable_account("mimo"), "mimo");
        assert_eq!(portable_account("profile/work/provider/mimo"), "mimo");
        assert_eq!(
            portable_account("profile/work/other/mimo"),
            "profile/work/other/mimo"
        );
    }

    #[test]
    fn keychain_references_are_exact_and_provider_scoped() {
        assert_eq!(
            credential_reference_provider(&credential_reference("deepseek")),
            Some("deepseek")
        );
        assert_eq!(
            profile_credential_reference_parts(&profile_credential_reference("work", "deepseek")),
            Some(("work", "deepseek"))
        );
        assert_eq!(
            keychain_reference_provider(&keychain_reference("deepseek")),
            Some("deepseek")
        );
        assert_eq!(keychain_reference_provider("keychain://ca/provider/"), None);
        assert_eq!(
            keychain_reference_provider("Bearer keychain://ca/provider/deepseek"),
            None
        );
        assert_eq!(
            keychain_reference_provider("keychain://ca/provider/bad provider"),
            None
        );
        assert!(resembles_keychain_reference(
            "keychain://ca/provider/bad provider"
        ));
        assert_eq!(
            profile_keychain_reference_parts(&profile_keychain_reference("work", "deepseek")),
            Some(("work", "deepseek"))
        );
        assert_eq!(
            profile_keychain_reference_parts("keychain://ca/profile/../provider/deepseek"),
            None
        );
        assert_eq!(
            keychain_reference_provider(&profile_keychain_reference("work", "deepseek")),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn portable_credentials_are_private_atomic_and_profile_scoped() {
        use std::os::unix::fs::MetadataExt as _;
        let temp = tempfile::tempdir().unwrap();
        let store = FileProviderSecretStore::new(temp.path().join("credentials"));

        store.store("deepseek", b"first").unwrap();
        store
            .store("profile/work/provider/deepseek", b"work")
            .unwrap();
        store.store("deepseek", b"replacement").unwrap();

        let default_path = temp.path().join("credentials/deepseek");
        let profile_path = temp
            .path()
            .join("credentials/profile/work/provider/deepseek");
        assert_eq!(store.load("deepseek").unwrap().unwrap(), b"replacement");
        assert_eq!(
            store
                .load("profile/work/provider/deepseek")
                .unwrap()
                .unwrap(),
            b"work"
        );
        assert_eq!(
            std::fs::metadata(default_path).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(profile_path).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(temp.path().join("credentials"))
                .unwrap()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(temp.path().join("credentials"))
                .unwrap()
                .uid(),
            unsafe { libc::geteuid() }
        );
        assert!(std::fs::read_dir(temp.path().join("credentials"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")));
    }

    #[cfg(unix)]
    #[test]
    fn portable_credentials_reject_symlinks_permissions_and_oversize() {
        use std::os::unix::fs::{symlink, PermissionsExt as _};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("credentials");
        let store = FileProviderSecretStore::new(root.clone());
        store.store("deepseek", b"secret").unwrap();

        let path = root.join("deepseek");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(store
            .load("deepseek")
            .unwrap_err()
            .to_string()
            .contains("0600"));
        std::fs::remove_file(&path).unwrap();
        let target = temp.path().join("target");
        std::fs::write(&target, b"elsewhere").unwrap();
        symlink(&target, &path).unwrap();
        assert!(store.load("deepseek").is_err());
        assert!(store.store("deepseek", b"replacement").is_err());
        assert!(store.delete("deepseek").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"elsewhere");
        assert!(store
            .store("openai", &vec![b'x'; MAX_SECRET_BYTES as usize + 1])
            .is_err());

        let nested_root = temp.path().join("nested-credentials");
        std::fs::create_dir(&nested_root).unwrap();
        std::fs::set_permissions(&nested_root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let redirected = temp.path().join("redirected");
        std::fs::create_dir(&redirected).unwrap();
        symlink(&redirected, nested_root.join("profile")).unwrap();
        let nested = FileProviderSecretStore::new(nested_root);
        assert!(nested
            .store("profile/work/provider/deepseek", b"secret")
            .is_err());
        assert!(std::fs::read_dir(&redirected).unwrap().next().is_none());

        let loose_root = temp.path().join("loose-credentials");
        std::fs::create_dir(&loose_root).unwrap();
        std::fs::set_permissions(&loose_root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let loose_file = loose_root.join("deepseek");
        std::fs::write(&loose_file, b"secret").unwrap();
        std::fs::set_permissions(&loose_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(FileProviderSecretStore::new(loose_root)
            .load("deepseek")
            .unwrap_err()
            .to_string()
            .contains("0700"));
    }

    #[test]
    fn memory_credentials_delete_and_remain_redacted() {
        let store = MemoryProviderSecretStore::default();
        store.store("openai", b"very-secret").unwrap();
        assert_eq!(store.load("openai").unwrap().unwrap(), b"very-secret");
        store.delete("openai").unwrap();
        assert_eq!(store.load("openai").unwrap(), None);
        assert!(!format!("{store:?}").contains("very-secret"));
    }
}
