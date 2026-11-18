//! Tool-path lookup before a read, list, edit, or search opens the path.
//!
//! A missing name can match one on-disk sibling that differs only by a
//! unicode space. A still-missing path gets a short hint. Canonicalize on
//! this path waits at most 30 seconds, then keeps the path the model sent.

use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const CANONICALIZE_LIMIT: Duration = Duration::from_secs(30);
pub const UNICODE_SCAN_LIMIT: Duration = Duration::from_secs(30);
pub const HINT_LIMIT: Duration = Duration::from_millis(100);

const MAX_SIMILAR: usize = 3;
const MIN_LEAF_LEN: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canon {
    Found(PathBuf),
    Missing,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    pub note: Option<String>,
    pub missing_hint: Option<String>,
}

pub fn locate_missing(path: &Path, cwd: &Path) -> Located {
    match unicode_space_match_until(path, UNICODE_SCAN_LIMIT) {
        Some(found) => Located {
            path: found.path,
            note: Some(found.note),
            missing_hint: None,
        },
        None => Located {
            path: path.to_path_buf(),
            note: None,
            missing_hint: Some(missing_hint_until(path, cwd, HINT_LIMIT)),
        },
    }
}

pub fn locate(path: &Path, cwd: &Path) -> Located {
    locate_with(
        path,
        cwd,
        |path| std::fs::canonicalize(path),
        CANONICALIZE_LIMIT,
        UNICODE_SCAN_LIMIT,
        HINT_LIMIT,
    )
}

pub fn canonicalize_until<F>(path: &Path, limit: Duration, canonicalize: F) -> Canon
where
    F: FnOnce(&Path) -> io::Result<PathBuf> + Send + 'static,
{
    let probe = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(canonicalize(&probe));
    });
    match rx.recv_timeout(limit) {
        Ok(Ok(found)) => Canon::Found(found),
        Ok(Err(error)) if error.kind() == ErrorKind::NotFound => Canon::Missing,
        Ok(Err(_)) | Err(_) => Canon::TimedOut,
    }
}

pub fn locate_with<F>(
    path: &Path,
    cwd: &Path,
    canonicalize: F,
    canonicalize_limit: Duration,
    unicode_limit: Duration,
    hint_limit: Duration,
) -> Located
where
    F: FnOnce(&Path) -> io::Result<PathBuf> + Send + 'static,
{
    match canonicalize_until(path, canonicalize_limit, canonicalize) {
        Canon::Found(found) => Located {
            path: found,
            note: None,
            missing_hint: None,
        },
        Canon::TimedOut => Located {
            path: path.to_path_buf(),
            note: None,
            missing_hint: None,
        },
        Canon::Missing => match unicode_space_match_until(path, unicode_limit) {
            Some(found) => Located {
                note: Some(found.note),
                path: found.path,
                missing_hint: None,
            },
            None => Located {
                path: path.to_path_buf(),
                missing_hint: Some(missing_hint_until(path, cwd, hint_limit)),
                note: None,
            },
        },
    }
}

struct UnicodeHit {
    path: PathBuf,
    note: String,
}

fn unicode_space_match_until(path: &Path, limit: Duration) -> Option<UnicodeHit> {
    let probe = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(unicode_space_match(&probe));
    });
    rx.recv_timeout(limit).ok().flatten()
}

fn unicode_space_match(path: &Path) -> Option<UnicodeHit> {
    let requested = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    let folded = fold_spaces(requested);
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(parent).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == requested || fold_spaces(name) != folded {
            continue;
        }
        hits.push((entry.path(), name.to_string()));
        if hits.len() > 1 {
            return None;
        }
    }
    let (found, actual) = hits.pop()?;
    let points = differing_points(&actual, requested);
    let listed = if points.is_empty() {
        "U+202F".to_string()
    } else {
        points.join(", ")
    };
    Some(UnicodeHit {
        path: found,
        note: format!("Resolved filename differs by {listed}."),
    })
}

fn fold_spaces(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '\u{202F}' | '\u{00A0}' => ' ',
            other => other,
        })
        .collect()
}

fn differing_points(actual: &str, requested: &str) -> Vec<String> {
    let mut points = Vec::new();
    for (actual, _) in actual.chars().zip(requested.chars()) {
        if matches!(actual, '\u{202F}' | '\u{00A0}') {
            let label = format!("U+{:04X}", actual as u32);
            if !points.contains(&label) {
                points.push(label);
            }
        }
    }
    points
}

fn missing_hint_until(path: &Path, cwd: &Path, limit: Duration) -> String {
    let path = path.to_path_buf();
    let cwd = cwd.to_path_buf();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(missing_hint(&path, &cwd));
    });
    rx.recv_timeout(limit).unwrap_or_default()
}

fn missing_hint(path: &Path, cwd: &Path) -> String {
    let mut hint = String::new();
    if let Some(corrected) = dropped_repo_folder(path, cwd) {
        hint.push_str(&format!(" Did you mean {}?", corrected.display()));
    } else {
        let names = similar_siblings(path);
        if !names.is_empty() {
            hint.push_str("\nSimilar entries in parent directory: ");
            hint.push_str(&names.join(", "));
        }
    }
    hint.push_str(&format!(
        "\nNote: your current working directory is {}",
        cwd.display()
    ));
    hint
}

fn dropped_repo_folder(path: &Path, cwd: &Path) -> Option<PathBuf> {
    let path = comparable(path);
    let cwd_key = comparable(cwd);
    if !path.is_absolute() || path.starts_with(&cwd_key) {
        return None;
    }
    let parent = cwd_key.parent()?;
    let relative = path.strip_prefix(parent).ok()?;
    if let Some(Component::Normal(first)) = relative.components().next() {
        let sibling = parent.join(first);
        if sibling != cwd_key && sibling.exists() {
            return None;
        }
    }
    let candidate = cwd.join(relative);
    candidate.exists().then_some(candidate)
}

fn comparable(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("/private/var/") {
        return PathBuf::from(format!("/var/{rest}"));
    }
    if let Some(rest) = text.strip_prefix("/private/tmp/") {
        return PathBuf::from(format!("/tmp/{rest}"));
    }
    path.to_path_buf()
}

fn similar_siblings(path: &Path) -> Vec<String> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Some(leaf) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let leaf_stem = stem(leaf);
    if leaf_stem.chars().count() < MIN_LEAF_LEN {
        return Vec::new();
    }
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.eq_ignore_ascii_case(leaf) {
            continue;
        }
        let name_stem = stem(name);
        if name_stem.contains(&leaf_stem) {
            names.push(name.to_string());
            if names.len() == MAX_SIMILAR {
                break;
            }
        }
    }
    names
}

fn stem(name: &str) -> String {
    Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name)
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_unicode_space_filename_resolves_when_it_is_the_only_match() {
        let temp = tempfile::tempdir().unwrap();
        let name = format!("Shot{}1.txt", '\u{202F}');
        std::fs::write(temp.path().join(&name), "hi").unwrap();
        let requested = temp.path().join("Shot 1.txt");
        let located = locate(&requested, temp.path());
        assert_eq!(located.path, temp.path().join(&name));
        assert_eq!(
            located.note.as_deref(),
            Some("Resolved filename differs by U+202F.")
        );
        assert!(located.missing_hint.is_none());
    }

    #[test]
    fn two_unicode_space_names_stay_missing() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(format!("Shot{}1.txt", '\u{202F}')), "a").unwrap();
        std::fs::write(temp.path().join(format!("Shot{}1.txt", '\u{00A0}')), "b").unwrap();
        let requested = temp.path().join("Shot 1.txt");
        let located = locate(&requested, temp.path());
        assert_eq!(located.path, requested);
        assert!(located.note.is_none());
        assert!(located.missing_hint.is_some());
    }

    #[test]
    fn a_dropped_repo_folder_is_named_when_the_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(repo.join("main.rs"), "fn main() {}").unwrap();
        let requested = temp.path().join("main.rs");
        let located = locate(&requested, &repo);
        let hint = located.missing_hint.expect("hint");
        assert!(hint.contains("Did you mean"), "{hint}");
        assert!(
            hint.contains("repo/main.rs") || hint.contains("repo\\main.rs"),
            "{hint}"
        );
        assert!(hint.contains("current working directory"), "{hint}");
    }

    #[test]
    fn a_missing_file_names_up_to_three_similar_siblings() {
        let temp = tempfile::tempdir().unwrap();
        for name in ["widget.rs", "widget_test.rs", "widget_old.rs", "other.rs"] {
            std::fs::write(temp.path().join(name), "x").unwrap();
        }
        let requested = temp.path().join("widg.rs");
        let located = locate(&requested, temp.path());
        let hint = located.missing_hint.expect("hint");
        assert!(hint.contains("widget.rs"), "{hint}");
        assert!(hint.contains("widget_test.rs"), "{hint}");
        assert!(hint.contains("widget_old.rs"), "{hint}");
        assert!(!hint.contains("other.rs"), "{hint}");
        assert!(hint.contains("current working directory"), "{hint}");
    }

    #[test]
    fn a_stalled_canonicalize_keeps_the_path_the_model_sent() {
        let path = PathBuf::from("/tmp/model-sent");
        let release = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&release);
        let located = locate_with(
            &path,
            Path::new("/tmp"),
            move |_| {
                while !flag.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(2));
                }
                Ok(PathBuf::from("/canonical"))
            },
            Duration::from_millis(20),
            Duration::from_millis(1),
            Duration::from_millis(1),
        );
        assert_eq!(located.path, path);
        assert!(located.note.is_none());
        assert!(located.missing_hint.is_none());
        release.store(true, Ordering::SeqCst);
    }

    #[test]
    fn a_nonbreaking_space_is_named_too() {
        let temp = tempfile::tempdir().unwrap();
        let name = format!("Notes{}1.txt", '\u{00A0}');
        std::fs::write(temp.path().join(&name), "n").unwrap();
        let located = locate(&temp.path().join("Notes 1.txt"), temp.path());
        assert_eq!(located.path, temp.path().join(name));
        assert_eq!(
            located.note.as_deref(),
            Some("Resolved filename differs by U+00A0.")
        );
    }
}
