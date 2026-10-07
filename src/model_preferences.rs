use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MAX_RECENT_MODELS: usize = 8;
pub const MAX_FAVORITE_MODELS: usize = 128;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_STORE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub provider: String,
    pub model: String,
}

impl ModelIdentity {
    pub fn from_model(model: &crate::app::ModelInfo) -> Self {
        Self {
            provider: model.provider.clone(),
            model: model.id.clone(),
        }
    }

    pub fn matches(&self, model: &crate::app::ModelInfo) -> bool {
        self.provider == model.provider && self.model == model.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentModel {
    pub identity: ModelIdentity,
    pub selected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPreferences {
    #[serde(default)]
    pub favorites: Vec<ModelIdentity>,
    #[serde(default)]
    pub recent: Vec<RecentModel>,
}

impl ModelPreferences {
    pub fn is_favorite(&self, model: &crate::app::ModelInfo) -> bool {
        self.favorites
            .iter()
            .any(|identity| identity.matches(model))
    }

    pub fn toggle_favorite(&mut self, model: &crate::app::ModelInfo) -> Result<bool, String> {
        validate_identity(&ModelIdentity::from_model(model))?;
        if let Some(index) = self
            .favorites
            .iter()
            .position(|identity| identity.matches(model))
        {
            self.favorites.remove(index);
            return Ok(false);
        }
        if self.favorites.len() >= MAX_FAVORITE_MODELS {
            return Err(format!(
                "model favorites are limited to {MAX_FAVORITE_MODELS} entries"
            ));
        }
        self.favorites.push(ModelIdentity::from_model(model));
        self.favorites.sort_by(|left, right| {
            (&left.provider, &left.model).cmp(&(&right.provider, &right.model))
        });
        Ok(true)
    }

    pub fn record_selection(
        &mut self,
        model: &crate::app::ModelInfo,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let identity = ModelIdentity::from_model(model);
        validate_identity(&identity)?;
        self.recent.retain(|entry| !entry.identity.matches(model));
        self.recent.insert(
            0,
            RecentModel {
                identity,
                selected_at: now,
            },
        );
        self.recent.truncate(MAX_RECENT_MODELS);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelPickerSection {
    Favorites,
    Recent,
    Provider(String),
}

impl ModelPickerSection {
    pub fn label(&self) -> &str {
        match self {
            Self::Favorites => "Favorites",
            Self::Recent => "Recent",
            Self::Provider(provider) => provider,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPickerRow {
    pub section: ModelPickerSection,
    pub model: crate::app::ModelInfo,
    pub favorite: bool,
}

pub fn model_matches_filter(model: &crate::app::ModelInfo, filter_lower: &str) -> bool {
    filter_lower.is_empty()
        || model.display_name.to_lowercase().contains(filter_lower)
        || model.provider.to_lowercase().contains(filter_lower)
}

pub fn picker_rows(
    models: &[crate::app::ModelInfo],
    preferences: &ModelPreferences,
    filter: &str,
) -> Vec<ModelPickerRow> {
    let filter_lower = filter.to_lowercase();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();

    for model in models {
        let identity = ModelIdentity::from_model(model);
        if preferences.is_favorite(model)
            && model_matches_filter(model, &filter_lower)
            && seen.insert(identity)
        {
            rows.push(ModelPickerRow {
                section: ModelPickerSection::Favorites,
                model: model.clone(),
                favorite: true,
            });
        }
    }
    for recent in &preferences.recent {
        let Some(model) = models.iter().find(|model| recent.identity.matches(model)) else {
            continue;
        };
        let identity = ModelIdentity::from_model(model);
        if model_matches_filter(model, &filter_lower) && seen.insert(identity) {
            rows.push(ModelPickerRow {
                section: ModelPickerSection::Recent,
                model: model.clone(),
                favorite: false,
            });
        }
    }
    for model in models {
        let identity = ModelIdentity::from_model(model);
        if model_matches_filter(model, &filter_lower) && seen.insert(identity) {
            rows.push(ModelPickerRow {
                section: ModelPickerSection::Provider(model.provider.clone()),
                model: model.clone(),
                favorite: false,
            });
        }
    }
    rows
}

pub fn available_recent_models(
    models: &[crate::app::ModelInfo],
    preferences: &ModelPreferences,
) -> Vec<crate::app::ModelInfo> {
    preferences
        .recent
        .iter()
        .filter_map(|entry| {
            models
                .iter()
                .find(|model| entry.identity.matches(model))
                .cloned()
        })
        .collect()
}

pub fn available_favorite_models(
    models: &[crate::app::ModelInfo],
    preferences: &ModelPreferences,
) -> Vec<crate::app::ModelInfo> {
    models
        .iter()
        .filter(|model| preferences.is_favorite(model))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    preferences: ModelPreferences,
}

#[derive(Debug, Clone)]
pub struct ModelPreferencesStore {
    path: PathBuf,
    writable: bool,
}

#[derive(Debug)]
enum LoadFailure {
    Corrupt(String),
    Unsafe(String),
}

impl LoadFailure {
    fn message(self) -> String {
        match self {
            Self::Corrupt(message) | Self::Unsafe(message) => message,
        }
    }
}

impl ModelPreferencesStore {
    pub fn for_profile(home: &Path, profile_data: &Path) -> Result<Self, String> {
        let relative = profile_data
            .strip_prefix(home)
            .map_err(|_| "model preference profile data is outside HOME".to_string())?;
        if relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err("model preference profile path is invalid".into());
        }
        let canonical_home = home
            .canonicalize()
            .map_err(|error| format!("could not resolve HOME for model preferences: {error}"))?;
        let canonical_profile_data = canonical_home.join(relative);
        validate_profile_components(&canonical_home, &canonical_profile_data)?;
        Ok(Self {
            path: canonical_profile_data.join("models/preferences.json"),
            writable: true,
        })
    }

    pub fn unavailable() -> Self {
        Self {
            path: PathBuf::new(),
            writable: false,
        }
    }

    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn load(&self) -> Result<ModelPreferences, String> {
        self.load_checked().map_err(LoadFailure::message)
    }

    pub fn save(&self, preferences: &ModelPreferences) -> Result<bool, String> {
        if !self.writable {
            return Err("model favorites are unavailable in bare mode".into());
        }
        validate_preferences(preferences)?;
        let recovered = match self.load_checked() {
            Ok(_) => false,
            Err(LoadFailure::Corrupt(_)) => {
                self.quarantine_corrupt()?;
                true
            }
            Err(error) => return Err(error.message()),
        };
        self.write(preferences)?;
        Ok(recovered)
    }

    fn load_checked(&self) -> Result<ModelPreferences, LoadFailure> {
        if self.path.as_os_str().is_empty() {
            return Ok(ModelPreferences::default());
        }
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ModelPreferences::default());
            }
            Err(error) => {
                return Err(LoadFailure::Unsafe(format!(
                    "could not inspect model preferences: {error}"
                )));
            }
        };
        validate_no_symlink_ancestors(&self.path).map_err(LoadFailure::Unsafe)?;
        if !metadata.file_type().is_file() {
            return Err(LoadFailure::Unsafe(
                "model preferences are not a regular file".into(),
            ));
        }
        validate_owner_only(&metadata).map_err(LoadFailure::Unsafe)?;
        if metadata.len() as usize > MAX_STORE_BYTES {
            return Err(LoadFailure::Unsafe(
                "model preferences exceed their bounded storage limit".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .map_err(|error| {
                LoadFailure::Unsafe(format!("could not open model preferences: {error}"))
            })?
            .take((MAX_STORE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                LoadFailure::Unsafe(format!("could not read model preferences: {error}"))
            })?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err(LoadFailure::Unsafe(
                "model preferences exceed their bounded storage limit".into(),
            ));
        }
        let document: Document = serde_json::from_slice(&bytes)
            .map_err(|_| LoadFailure::Corrupt("model preference data is invalid".into()))?;
        if document.version != 1 {
            return Err(LoadFailure::Corrupt(
                "model preference version is unsupported".into(),
            ));
        }
        validate_preferences(&document.preferences).map_err(LoadFailure::Corrupt)?;
        Ok(document.preferences)
    }

    fn write(&self, preferences: &ModelPreferences) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&Document {
            version: 1,
            preferences: preferences.clone(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_STORE_BYTES {
            return Err("model preferences exceed their bounded storage limit".into());
        }
        let parent = self
            .path
            .parent()
            .ok_or("model preference path has no parent")?;
        validate_no_symlink_ancestors(parent)?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        validate_no_symlink_ancestors(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err("model preferences are not a regular file".into());
            }
            Ok(metadata) => validate_owner_only(&metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let temp = parent.join(format!(".model-preferences-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        let result = (|| {
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&temp, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn quarantine_corrupt(&self) -> Result<(), String> {
        validate_no_symlink_ancestors(&self.path)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.len() as usize > MAX_STORE_BYTES {
            return Err("unsafe model preferences cannot be recovered automatically".into());
        }
        validate_owner_only(&metadata)?;
        let parent = self
            .path
            .parent()
            .ok_or("model preference path has no parent")?;
        let quarantine = parent.join(format!(
            ".model-preferences-corrupt-{}.json",
            uuid::Uuid::new_v4()
        ));
        fs::rename(&self.path, quarantine).map_err(|error| error.to_string())
    }
}

fn validate_identity(identity: &ModelIdentity) -> Result<(), String> {
    if identity.provider.trim().is_empty() || identity.model.trim().is_empty() {
        return Err("model preference identity cannot be blank".into());
    }
    let length = identity.provider.len().saturating_add(identity.model.len());
    if length > MAX_IDENTITY_BYTES {
        return Err("model preference identity is too long".into());
    }
    if identity
        .provider
        .chars()
        .chain(identity.model.chars())
        .any(char::is_control)
    {
        return Err("model preference identity contains a control character".into());
    }
    Ok(())
}

fn validate_preferences(preferences: &ModelPreferences) -> Result<(), String> {
    if preferences.favorites.len() > MAX_FAVORITE_MODELS {
        return Err(format!(
            "model favorites exceed {MAX_FAVORITE_MODELS} entries"
        ));
    }
    if preferences.recent.len() > MAX_RECENT_MODELS {
        return Err(format!("recent models exceed {MAX_RECENT_MODELS} entries"));
    }
    let mut favorites = HashSet::new();
    for identity in &preferences.favorites {
        validate_identity(identity)?;
        if !favorites.insert(identity) {
            return Err("model favorites contain a duplicate identity".into());
        }
    }
    let mut recent = HashSet::new();
    for entry in &preferences.recent {
        validate_identity(&entry.identity)?;
        if !recent.insert(&entry.identity) {
            return Err("recent models contain a duplicate identity".into());
        }
    }
    Ok(())
}

fn validate_no_symlink_ancestors(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("model preference path cannot contain symlinks".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("could not inspect model preference path: {error}"));
            }
        }
    }
    Ok(())
}

fn validate_profile_components(home: &Path, profile_data: &Path) -> Result<(), String> {
    let relative = profile_data
        .strip_prefix(home)
        .map_err(|_| "model preference profile data is outside canonical HOME".to_string())?;
    let mut current = home.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("model preference profile path cannot contain symlinks".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect model preference profile: {error}"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_owner_only(metadata: &fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err("model preferences are not owner-only".into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_owner_only(_metadata: &fs::Metadata) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn model(provider: &str, id: &str) -> crate::app::ModelInfo {
        crate::app::ModelInfo {
            provider: provider.into(),
            id: id.into(),
            display_name: id.into(),
        }
    }

    fn at(second: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + second, 0).unwrap()
    }

    fn store(root: &tempfile::TempDir) -> ModelPreferencesStore {
        ModelPreferencesStore::for_profile(root.path(), &root.path().join("profile")).unwrap()
    }

    #[test]
    fn recents_are_unique_newest_first_and_bounded() {
        let mut preferences = ModelPreferences::default();
        for index in 0..10 {
            preferences
                .record_selection(&model("p", &format!("m{index}")), at(index))
                .unwrap();
        }
        preferences
            .record_selection(&model("p", "m5"), at(20))
            .unwrap();
        assert_eq!(preferences.recent.len(), MAX_RECENT_MODELS);
        assert_eq!(preferences.recent[0].identity.model, "m5");
        assert_eq!(
            preferences
                .recent
                .iter()
                .filter(|entry| entry.identity.model == "m5")
                .count(),
            1
        );
        assert!(!preferences
            .recent
            .iter()
            .any(|entry| entry.identity.model == "m0"));
    }

    #[test]
    fn favorite_count_and_identity_validation_are_bounded() {
        let mut preferences = ModelPreferences::default();
        for index in 0..MAX_FAVORITE_MODELS {
            preferences
                .toggle_favorite(&model("p", &format!("m{index}")))
                .unwrap();
        }
        assert!(preferences
            .toggle_favorite(&model("p", "one-too-many"))
            .unwrap_err()
            .contains("limited"));
        assert!(preferences
            .toggle_favorite(&model("p", "bad\nmodel"))
            .unwrap_err()
            .contains("control"));
    }

    #[test]
    fn picker_sections_are_unique_and_hide_unavailable_preferences() {
        let models = vec![model("a", "one"), model("a", "two"), model("b", "three")];
        let mut preferences = ModelPreferences::default();
        preferences.toggle_favorite(&models[1]).unwrap();
        preferences.favorites.push(ModelIdentity {
            provider: "gone".into(),
            model: "hidden".into(),
        });
        preferences.record_selection(&models[0], at(1)).unwrap();
        preferences.record_selection(&models[1], at(2)).unwrap();
        preferences.recent.push(RecentModel {
            identity: ModelIdentity {
                provider: "gone".into(),
                model: "hidden".into(),
            },
            selected_at: at(3),
        });

        let rows = picker_rows(&models, &preferences, "");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].section, ModelPickerSection::Favorites);
        assert_eq!(rows[0].model.id, "two");
        assert_eq!(rows[1].section, ModelPickerSection::Recent);
        assert_eq!(rows[1].model.id, "one");
        assert_eq!(rows[2].section, ModelPickerSection::Provider("b".into()));
        assert_eq!(
            rows.iter()
                .map(|row| ModelIdentity::from_model(&row.model))
                .collect::<HashSet<_>>()
                .len(),
            rows.len()
        );
    }

    #[test]
    fn unavailable_favorite_reappears_when_the_model_returns() {
        let mut preferences = ModelPreferences::default();
        let returning = model("p", "returning");
        preferences.toggle_favorite(&returning).unwrap();
        assert!(picker_rows(&[], &preferences, "").is_empty());
        let rows = picker_rows(std::slice::from_ref(&returning), &preferences, "");
        assert_eq!(rows[0].section, ModelPickerSection::Favorites);
    }

    #[test]
    fn store_round_trips_owner_only_preferences() {
        let root = tempfile::tempdir().unwrap();
        let store = store(&root);
        let mut preferences = ModelPreferences::default();
        let selected = model("p", "m");
        preferences.toggle_favorite(&selected).unwrap();
        preferences.record_selection(&selected, at(1)).unwrap();
        assert!(!store.save(&preferences).unwrap());
        assert_eq!(store.load().unwrap(), preferences);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&store.path).unwrap().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_store_is_recovered_only_by_an_explicit_save() {
        let root = tempfile::tempdir().unwrap();
        let store = store(&root);
        fs::create_dir_all(store.path.parent().unwrap()).unwrap();
        fs::write(&store.path, b"not json").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(store.load().is_err());
        assert!(store.save(&ModelPreferences::default()).unwrap());
        assert_eq!(store.load().unwrap(), ModelPreferences::default());
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().contains("corrupt"))
                .count(),
            1
        );
    }

    #[test]
    fn unavailable_store_never_persists_favorites() {
        let store = ModelPreferencesStore::unavailable();
        assert!(!store.writable());
        assert!(store.save(&ModelPreferences::default()).is_err());
        assert_eq!(store.load().unwrap(), ModelPreferences::default());
    }

    #[cfg(unix)]
    #[test]
    fn profile_owned_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("profile")).unwrap();
        let error = ModelPreferencesStore::for_profile(root.path(), &root.path().join("profile"))
            .unwrap_err();
        assert!(error.contains("symlink"));
    }
}
