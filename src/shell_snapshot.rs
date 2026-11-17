//! Persistent shell snapshot and secret environment filter.
//!
//! The replay script restores cwd, exports, functions, aliases, and options so
//! a later command sees them. This is not `/term`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Names kept before include globs, unless a secret pattern or exclude drops them.
pub const CORE_ENV_NAMES: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR",
    "TMP", "TEMP", "PWD",
];

const SECRET_MARKERS: &[&str] = &["key", "secret", "token"];

/// Explicit include and exclude globs. `*` is the only wildcard.
///
/// Exclude wins. Include then wins over the secret-name drop and the core set.
/// Globs are case-sensitive. Secret markers are not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretPolicy {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// Values a later shell command should observe. Exports are already chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSnapshot {
    pub cwd: PathBuf,
    pub exports: BTreeMap<String, String>,
    pub functions: BTreeMap<String, String>,
    pub aliases: BTreeMap<String, String>,
    pub options: BTreeSet<String>,
}

impl ShellSnapshot {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            exports: BTreeMap::new(),
            functions: BTreeMap::new(),
            aliases: BTreeMap::new(),
            options: BTreeSet::new(),
        }
    }
}

/// Keep core names that are not secret-shaped, plus includes, minus excludes.
#[must_use]
pub fn env_allowed(name: &str, policy: &SecretPolicy) -> bool {
    if policy
        .exclude
        .iter()
        .any(|pattern| glob_match(pattern, name))
    {
        return false;
    }
    if policy
        .include
        .iter()
        .any(|pattern| glob_match(pattern, name))
    {
        return true;
    }
    if secret_name(name) {
        return false;
    }
    CORE_ENV_NAMES.contains(&name)
}

/// Filter `env` with [`env_allowed`]. Non-core names are dropped.
#[must_use]
pub fn filter_env(
    env: &BTreeMap<String, String>,
    policy: &SecretPolicy,
) -> BTreeMap<String, String> {
    env.iter()
        .filter(|(name, _)| env_allowed(name, policy))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// Apply direnv deltas after a filter, then filter the merged map again.
///
/// `None` removes a name. Secret-shaped delta names are dropped unless included.
#[must_use]
pub fn apply_direnv(
    mut filtered: BTreeMap<String, String>,
    deltas: &BTreeMap<String, Option<String>>,
    policy: &SecretPolicy,
) -> BTreeMap<String, String> {
    for (name, value) in deltas {
        match value {
            Some(value) => {
                filtered.insert(name.clone(), value.clone());
            }
            None => {
                filtered.remove(name);
            }
        }
    }
    filtered.retain(|name, _| env_allowed(name, policy));
    filtered
}

/// Script that restores this snapshot. Unsafe shell names are an error.
pub fn replay_script(snapshot: &ShellSnapshot) -> Result<String, String> {
    let cwd = path_utf8(&snapshot.cwd)?;
    if cwd.is_empty() {
        return Err("shell snapshot cwd is empty".into());
    }
    let mut script = String::new();
    script.push_str("cd ");
    script.push_str(&shell_quote(cwd));
    script.push('\n');
    for option in &snapshot.options {
        require_shell_name(option, "shell option")?;
        script.push_str("set -o ");
        script.push_str(option);
        script.push('\n');
    }
    for (name, value) in &snapshot.exports {
        require_shell_name(name, "export")?;
        script.push_str("export ");
        script.push_str(name);
        script.push('=');
        script.push_str(&shell_quote(value));
        script.push('\n');
    }
    for (name, value) in &snapshot.aliases {
        require_shell_name(name, "alias")?;
        script.push_str("alias ");
        script.push_str(name);
        script.push('=');
        script.push_str(&shell_quote(value));
        script.push('\n');
    }
    for (name, body) in &snapshot.functions {
        require_shell_name(name, "function")?;
        script.push_str(name);
        script.push_str("() {\n");
        script.push_str(body);
        if !body.ends_with('\n') {
            script.push('\n');
        }
        script.push_str("}\n");
    }
    Ok(script)
}

/// `command` prefixed by [`replay_script`], so the command sees the snapshot.
pub fn prepared_shell_command(command: &str) -> String {
    active_snapshot()
        .and_then(|snapshot| prefix_command(&snapshot, command).ok())
        .unwrap_or_else(|| command.to_string())
}

fn active_snapshot() -> Option<ShellSnapshot> {
    ACTIVE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

pub fn remember_snapshot(snapshot: ShellSnapshot) {
    *ACTIVE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(snapshot);
}

static ACTIVE: std::sync::OnceLock<std::sync::Mutex<Option<ShellSnapshot>>> =
    std::sync::OnceLock::new();

pub fn prefix_command(snapshot: &ShellSnapshot, command: &str) -> Result<String, String> {
    if command.is_empty() {
        return Err("shell command is empty".into());
    }
    let replay = replay_script(snapshot)?;
    Ok(format!("{replay}{command}\n"))
}

fn secret_name(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    SECRET_MARKERS.iter().any(|marker| folded.contains(marker))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let mut pattern_index = 0usize;
    let mut text_index = 0usize;
    let mut star_pattern: Option<usize> = None;
    let mut star_text = 0usize;
    while text_index < text.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == text[text_index] {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_pattern = Some(pattern_index);
            star_text = text_index;
            pattern_index += 1;
        } else if let Some(star) = star_pattern {
            star_text += 1;
            text_index = star_text;
            pattern_index = star + 1;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn require_shell_name(name: &str, kind: &str) -> Result<(), String> {
    if is_shell_name(name) {
        Ok(())
    } else {
        Err(format!("{kind} name is not a shell identifier: {name}"))
    }
}

fn is_shell_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn path_utf8(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("shell snapshot path is not utf-8: {}", path.display()))
}

fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '/' | '.' | ':'));
    if safe {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_direnv, filter_env, prefix_command, prepared_shell_command, remember_snapshot,
        replay_script, SecretPolicy, ShellSnapshot,
    };
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn policy(include: &[&str], exclude: &[&str]) -> SecretPolicy {
        SecretPolicy {
            include: include.iter().map(|item| (*item).to_string()).collect(),
            exclude: exclude.iter().map(|item| (*item).to_string()).collect(),
        }
    }

    #[test]
    fn openai_api_key_is_dropped() {
        let env = BTreeMap::from([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("PWD".to_string(), "/work".to_string()),
            ("OPENAI_API_KEY".to_string(), "sk-test".to_string()),
            ("MY_TOKEN".to_string(), "tok".to_string()),
            ("DB_SECRET".to_string(), "hide".to_string()),
            ("FOO".to_string(), "bar".to_string()),
        ]);
        let filtered = filter_env(&env, &SecretPolicy::default());
        assert_eq!(filtered.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(filtered.get("PWD").map(String::as_str), Some("/work"));
        assert!(!filtered.contains_key("OPENAI_API_KEY"));
        assert!(!filtered.contains_key("MY_TOKEN"));
        assert!(!filtered.contains_key("DB_SECRET"));
        assert!(!filtered.contains_key("FOO"));
        assert!(!format!("{filtered:?}").contains("sk-test"));
    }

    #[test]
    fn include_glob_keeps_a_named_key() {
        let env = BTreeMap::from([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("OPENAI_API_KEY".to_string(), "sk-live".to_string()),
        ]);
        let filtered = filter_env(&env, &policy(&["*API_KEY"], &[]));
        assert_eq!(
            filtered.get("OPENAI_API_KEY").map(String::as_str),
            Some("sk-live")
        );
        let excluded = filter_env(&env, &policy(&["OPENAI_*"], &["OPENAI_API_KEY"]));
        assert!(!excluded.contains_key("OPENAI_API_KEY"));
    }

    #[test]
    fn direnv_deltas_are_filtered_too() {
        let env = BTreeMap::from([
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/Users/old".to_string()),
            ("OPENAI_API_KEY".to_string(), "sk-base".to_string()),
        ]);
        let deltas = BTreeMap::from([
            ("OPENAI_API_KEY".to_string(), Some("sk-direnv".to_string())),
            ("HOME".to_string(), Some("/Users/new".to_string())),
            ("FOO".to_string(), Some("bar".to_string())),
            ("PATH".to_string(), None),
        ]);
        let plain = SecretPolicy::default();
        let merged = apply_direnv(filter_env(&env, &plain), &deltas, &plain);
        assert!(!merged.contains_key("OPENAI_API_KEY"));
        assert!(!merged.contains_key("FOO"));
        assert!(!merged.contains_key("PATH"));
        assert_eq!(merged.get("HOME").map(String::as_str), Some("/Users/new"));

        let kept_policy = policy(&["*API_KEY"], &[]);
        let kept = apply_direnv(filter_env(&env, &kept_policy), &deltas, &kept_policy);
        assert_eq!(
            kept.get("OPENAI_API_KEY").map(String::as_str),
            Some("sk-direnv")
        );
    }

    #[test]
    fn prepared_shell_command_replays_the_remembered_snapshot() {
        let mut snapshot = ShellSnapshot::new("/tmp/work");
        snapshot.exports.insert("FOO".into(), "bar".into());
        remember_snapshot(snapshot);
        let command = prepared_shell_command("printf '%s' \"$FOO\"");
        assert!(command.contains("cd "));
        assert!(command.contains("export FOO="));
        assert!(command.contains("printf '%s' \"$FOO\""));
        let mut env = std::collections::BTreeMap::new();
        env.insert("PATH".into(), "/bin".into());
        env.insert("OPENAI_API_KEY".into(), "sk-test".into());
        let filtered = filter_env(&env, &SecretPolicy::default());
        assert!(!filtered.contains_key("OPENAI_API_KEY"));
        assert!(!command.contains("sk-test"));
    }

    #[test]
    fn replay_prefixes_the_next_command_with_cd_and_export() {
        let mut snapshot = ShellSnapshot::new(PathBuf::from("/tmp"));
        snapshot
            .exports
            .insert("FOO".to_string(), "bar".to_string());
        snapshot.options.insert("pipefail".to_string());
        snapshot
            .aliases
            .insert("ll".to_string(), "ls -l".to_string());
        snapshot
            .functions
            .insert("greet".to_string(), "echo hi".to_string());

        let replay = replay_script(&snapshot).expect("replay");
        assert!(replay.contains("cd /tmp"));
        assert!(replay.contains("export FOO=bar"));
        assert!(replay.contains("set -o pipefail"));
        assert!(replay.contains("alias ll='ls -l'"));
        assert!(replay.contains("greet() {\necho hi\n}\n"));

        let second = prefix_command(&snapshot, "echo second").expect("prefix");
        assert!(second.starts_with(&replay));
        assert!(second[replay.len()..].contains("echo second"));
        assert!(prefix_command(&snapshot, "").is_err());
    }
}
