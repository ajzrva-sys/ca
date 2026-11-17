//! Tool-path refusal for credential files. This is not a sandbox and it does
//! not replace the provider filter. The check matches path patterns and does
//! not open the file.

use std::path::{Component, Path, PathBuf};

pub const REFUSAL: &str = "Refused: the provider client consumes credentials in this file.";

const CREDENTIAL_NAME_MARKERS: &[&str] = &["credential", "secret", "token", "passwd", "password"];

const CREDENTIAL_EXACT: &[&str] = &[
    "auth.json",
    "credentials",
    "credentials.json",
    "id_token",
    "access_token",
];

const CREDENTIAL_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx"];

/// Refuse `~/.config/ca` credential files and any `.env` file.
/// `.env.example`, `.env.sample`, and `.env.template` stay readable.
pub fn refusal(path: &Path) -> Option<&'static str> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    refusal_with_home(path, home.as_deref())
}

pub fn refusal_with_home(path: &Path, home: Option<&Path>) -> Option<&'static str> {
    let normalized = normalize(path);
    let name = normalized.file_name().and_then(|name| name.to_str())?;
    if is_denied_env_file(name) {
        return Some(REFUSAL);
    }
    if home.is_some_and(|home| under_ca_config(&normalized, home)) && looks_like_credential(name) {
        return Some(REFUSAL);
    }
    None
}

fn normalize(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(parent) = parent.canonicalize() {
            return parent.join(name);
        }
    }
    path.to_path_buf()
}

fn is_denied_env_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == ".env.example" || lower == ".env.sample" || lower == ".env.template" {
        return false;
    }
    if lower.ends_with(".example") {
        return false;
    }
    lower == ".env" || lower.starts_with(".env.")
}

fn looks_like_credential(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if CREDENTIAL_EXACT.iter().any(|exact| *exact == lower) {
        return true;
    }
    if CREDENTIAL_NAME_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    lower
        .rsplit_once('.')
        .is_some_and(|(_, extension)| CREDENTIAL_EXTENSIONS.contains(&extension))
}

fn under_ca_config(path: &Path, home: &Path) -> bool {
    let mut expected = home.join(".config").join("ca");
    if let Ok(canonical) = expected.canonicalize() {
        expected = canonical;
    }
    let mut components = path.components();
    for expected in expected.components() {
        if components.next() != Some(expected) {
            return false;
        }
    }
    matches!(components.next(), Some(Component::Normal(_)) | None) || path.starts_with(&expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_files_are_refused_and_examples_stay_readable() {
        let root = std::env::temp_dir();
        let env = root.join(".env");
        let local = root.join(".env.local");
        let example = root.join(".env.example");
        let sample = root.join(".env.sample");
        assert_eq!(refusal_with_home(&env, None), Some(REFUSAL));
        assert_eq!(refusal_with_home(&local, None), Some(REFUSAL));
        assert_eq!(refusal_with_home(&example, None), None);
        assert_eq!(refusal_with_home(&sample, None), None);
        assert_eq!(
            refusal_with_home(&root.join("src").join("main.rs"), None),
            None
        );
    }

    #[test]
    fn ca_config_credentials_are_refused_without_opening_them() {
        let home = std::env::temp_dir().join("ca-credential-home-test");
        let secret = home.join(".config").join("ca").join("credentials.json");
        let config = home.join(".config").join("ca").join("config.toml");
        let note = home.join("project").join("note.rs");
        assert_eq!(refusal_with_home(&secret, Some(&home)), Some(REFUSAL));
        assert_eq!(
            refusal_with_home(
                &home.join(".config").join("ca").join("api.token"),
                Some(&home)
            ),
            Some(REFUSAL)
        );
        assert_eq!(refusal_with_home(&config, Some(&home)), None);
        assert_eq!(refusal_with_home(&note, Some(&home)), None);
    }
}
