use crate::runtime::ActiveProfile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub const SCHEMA: &str = "ca.config-provenance.v1";
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_SELECTOR_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    BuiltInDefault,
    ExplicitProfileFile,
    ProcessOverride,
    EnvironmentReference,
    KeychainReference,
    DerivedRuntimeValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigProvenanceEntry {
    pub selector: String,
    pub source: ConfigSource,
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigProvenanceReport {
    pub schema: String,
    pub profile: String,
    pub entries: Vec<ConfigProvenanceEntry>,
}

impl ConfigProvenanceReport {
    pub fn filtered(&self, selector: Option<&str>) -> Result<Self, ProvenanceError> {
        let Some(selector) = selector else {
            return Ok(self.clone());
        };
        validate_selector(selector)?;
        let prefix = format!("{selector}.");
        let entries = self
            .entries
            .iter()
            .filter(|entry| entry.selector == selector || entry.selector.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Err(ProvenanceError::UnknownSelector);
        }
        Ok(Self {
            schema: self.schema.clone(),
            profile: self.profile.clone(),
            entries,
        })
    }

    pub fn render_text(&self) -> String {
        let mut output = format!("Configuration provenance · profile {}\n\n", self.profile);
        for entry in &self.entries {
            output.push_str(&format!(
                "{}\t{}\t{}\n",
                entry.selector,
                source_label(entry.source),
                if entry.present { "present" } else { "absent" }
            ));
        }
        output.trim_end().to_string()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProvenanceError {
    #[error("configuration storage is unsafe")]
    UnsafeStorage,
    #[error("configuration could not be read safely")]
    Read,
    #[error("configuration provenance could not be encoded")]
    Encode,
    #[error("configuration selector is invalid")]
    InvalidSelector,
    #[error("configuration selector was not found")]
    UnknownSelector,
    #[error("configuration provenance exceeds its entry bound")]
    TooManyEntries,
}

pub fn inspect(
    config_path: &Path,
    profile: &ActiveProfile,
    process_overrides: impl IntoIterator<Item = String>,
    derived_selectors: impl IntoIterator<Item = String>,
) -> Result<ConfigProvenanceReport, ProvenanceError> {
    let defaults = toml::Value::try_from(crate::config::CaConfig::default_config())
        .map_err(|_| ProvenanceError::Encode)?;
    let mut default_leaves = BTreeMap::new();
    flatten(None, &defaults, &mut default_leaves)?;

    let explicit = read_explicit(config_path)?;
    let mut explicit_leaves = BTreeMap::new();
    if let Some(explicit) = explicit.as_ref() {
        flatten(None, explicit, &mut explicit_leaves)?;
    }

    let process_overrides = process_overrides.into_iter().collect::<BTreeSet<_>>();
    for selector in &process_overrides {
        validate_selector(selector)?;
    }
    let derived_selectors = derived_selectors.into_iter().collect::<BTreeSet<_>>();
    for selector in &derived_selectors {
        validate_selector(selector)?;
    }

    let mut selectors = default_leaves.keys().cloned().collect::<BTreeSet<_>>();
    selectors.extend(explicit_leaves.keys().cloned());
    selectors.extend(process_overrides.iter().cloned());
    selectors.extend(derived_selectors.iter().cloned());
    if selectors.len() > MAX_ENTRIES {
        return Err(ProvenanceError::TooManyEntries);
    }

    let entries = selectors
        .into_iter()
        .map(|selector| {
            let explicit_value = explicit_leaves.get(&selector);
            let value = explicit_value.or_else(|| default_leaves.get(&selector));
            let source = if process_overrides.contains(&selector) {
                ConfigSource::ProcessOverride
            } else if derived_selectors.contains(&selector) {
                ConfigSource::DerivedRuntimeValue
            } else if let Some(value) = explicit_value {
                reference_source(value).unwrap_or(ConfigSource::ExplicitProfileFile)
            } else {
                value
                    .and_then(reference_source)
                    .unwrap_or(ConfigSource::BuiltInDefault)
            };
            let present = value.is_some_and(value_is_present)
                || process_overrides.contains(&selector)
                || derived_selectors.contains(&selector);
            ConfigProvenanceEntry {
                selector,
                source,
                present,
            }
        })
        .collect();
    Ok(ConfigProvenanceReport {
        schema: SCHEMA.into(),
        profile: profile.label().into(),
        entries,
    })
}

fn read_explicit(path: &Path) -> Result<Option<toml::Value>, ProvenanceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ProvenanceError::Read),
    };
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
        || metadata.len() > MAX_CONFIG_BYTES
    {
        return Err(ProvenanceError::UnsafeStorage);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| ProvenanceError::Read)?;
    let opened = file.metadata().map_err(|_| ProvenanceError::Read)?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(ProvenanceError::UnsafeStorage);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ProvenanceError::Read)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ProvenanceError::Read);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| ProvenanceError::Read)?;
    toml::from_str(text)
        .map(Some)
        .map_err(|_| ProvenanceError::Read)
}

fn flatten(
    prefix: Option<&str>,
    value: &toml::Value,
    output: &mut BTreeMap<String, toml::Value>,
) -> Result<(), ProvenanceError> {
    if let toml::Value::Table(table) = value {
        for (key, value) in table {
            let selector = prefix.map_or_else(|| key.clone(), |prefix| format!("{prefix}.{key}"));
            validate_selector(&selector)?;
            flatten(Some(&selector), value, output)?;
        }
        return Ok(());
    }
    let Some(selector) = prefix else {
        return Err(ProvenanceError::InvalidSelector);
    };
    if output.len() >= MAX_ENTRIES {
        return Err(ProvenanceError::TooManyEntries);
    }
    output.insert(selector.to_string(), value.clone());
    Ok(())
}

fn validate_selector(selector: &str) -> Result<(), ProvenanceError> {
    if selector.is_empty()
        || selector.len() > MAX_SELECTOR_BYTES
        || selector.starts_with('.')
        || selector.ends_with('.')
        || selector.contains("..")
        || !selector
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ProvenanceError::InvalidSelector);
    }
    Ok(())
}

fn reference_source(value: &toml::Value) -> Option<ConfigSource> {
    let value = value.as_str()?.trim();
    if exact_environment_reference(value) {
        Some(ConfigSource::EnvironmentReference)
    } else if crate::provider_secrets::resembles_saved_reference(value) {
        Some(ConfigSource::KeychainReference)
    } else {
        None
    }
}

fn exact_environment_reference(value: &str) -> bool {
    let Some(environment) = value
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    let mut characters = environment.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn value_is_present(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(value) => !value.is_empty(),
        toml::Value::Array(value) => !value.is_empty(),
        _ => true,
    }
}

fn source_label(source: ConfigSource) -> &'static str {
    match source {
        ConfigSource::BuiltInDefault => "built-in default",
        ConfigSource::ExplicitProfileFile => "explicit profile file",
        ConfigSource::ProcessOverride => "process override",
        ConfigSource::EnvironmentReference => "environment reference",
        ConfigSource::KeychainReference => "Keychain reference",
        ConfigSource::DerivedRuntimeValue => "derived runtime value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn private_config(root: &Path, text: &str) -> std::path::PathBuf {
        let path = root.join("config.toml");
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    #[test]
    fn reports_sources_without_values_or_reference_names() {
        let root = tempfile::tempdir().unwrap();
        let path = private_config(
            root.path(),
            r#"[defaults]
provider = "private-provider-canary"
model = "private-model-canary"
[providers.safe]
base_url = "https://secret-host-canary.invalid/v1"
api_key = "${SECRET_ENV_CANARY}"
models = ["private-model-canary"]
"#,
        );
        let report = inspect(
            &path,
            &ActiveProfile::Default,
            ["agent.sandbox_mode".to_string()],
            ["runtime.workspace".to_string()],
        )
        .unwrap();
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(!encoded.contains("private-provider-canary"));
        assert!(!encoded.contains("private-model-canary"));
        assert!(!encoded.contains("secret-host-canary"));
        assert!(!encoded.contains("SECRET_ENV_CANARY"));
        let api_key = report
            .entries
            .iter()
            .find(|entry| entry.selector == "providers.safe.api_key")
            .unwrap();
        assert_eq!(api_key.source, ConfigSource::EnvironmentReference);
        assert!(report.entries.iter().any(|entry| {
            entry.selector == "agent.sandbox_mode" && entry.source == ConfigSource::ProcessOverride
        }));
        assert!(report.entries.iter().any(|entry| {
            entry.selector == "runtime.workspace"
                && entry.source == ConfigSource::DerivedRuntimeValue
        }));
    }

    #[test]
    fn selector_filter_is_exact_or_descendant_only() {
        let report = ConfigProvenanceReport {
            schema: SCHEMA.into(),
            profile: "default".into(),
            entries: vec![
                ConfigProvenanceEntry {
                    selector: "agent.sandbox_mode".into(),
                    source: ConfigSource::BuiltInDefault,
                    present: true,
                },
                ConfigProvenanceEntry {
                    selector: "agentic.value".into(),
                    source: ConfigSource::BuiltInDefault,
                    present: true,
                },
            ],
        };
        let filtered = report.filtered(Some("agent")).unwrap();
        assert_eq!(filtered.entries.len(), 1);
        assert!(matches!(
            report.filtered(Some("missing")),
            Err(ProvenanceError::UnknownSelector)
        ));
    }

    #[test]
    fn refuses_symlink_hardlink_and_loose_modes() {
        let root = tempfile::tempdir().unwrap();
        let real = private_config(root.path(), "[defaults]\nprovider = 'safe'\n");
        let link = root.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            inspect(&link, &ActiveProfile::Default, [], []),
            Err(ProvenanceError::UnsafeStorage)
        );
        let hard = root.path().join("hard.toml");
        std::fs::hard_link(&real, &hard).unwrap();
        assert_eq!(
            inspect(&real, &ActiveProfile::Default, [], []),
            Err(ProvenanceError::UnsafeStorage)
        );
        std::fs::remove_file(hard).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            inspect(&real, &ActiveProfile::Default, [], []),
            Err(ProvenanceError::UnsafeStorage)
        );
    }
}
