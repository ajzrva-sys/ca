//! After a read, list, or edit, name one instruction file that was not in the
//! startup prompt. The reminder names the file and does not include its body.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const INSTRUCTION_NAMES: &[&str] = &["AGENTS.md", "CLAUDE.md"];

#[derive(Debug, Clone)]
pub struct InstructionProximity {
    roots: Vec<PathBuf>,
    known: HashSet<PathBuf>,
    reminded: HashSet<PathBuf>,
}

impl InstructionProximity {
    pub fn new(roots: &[PathBuf], startup: &[PathBuf]) -> Self {
        let mut known = HashSet::new();
        for path in startup {
            known.insert(normalize(path));
        }
        Self {
            roots: roots.iter().map(|path| normalize(path)).collect(),
            known,
            reminded: HashSet::new(),
        }
    }

    pub fn remember(&mut self, files: &[PathBuf]) {
        for path in files {
            self.known.insert(normalize(path));
        }
    }

    pub fn after_touch(&mut self, path: &Path) -> Option<String> {
        let touched = normalize(path);
        let start = if touched.is_dir() {
            touched
        } else {
            touched.parent().map(Path::to_path_buf)?
        };
        let root = self
            .roots
            .iter()
            .filter(|root| start.starts_with(root))
            .max_by_key(|root| root.as_os_str().len())?;
        let mut directory = Some(start);
        while let Some(current) = directory {
            if !current.starts_with(root) {
                break;
            }
            for name in INSTRUCTION_NAMES {
                let candidate = normalize(&current.join(name));
                if !candidate.is_file() || self.known.contains(&candidate) {
                    continue;
                }
                if ignored(root, &candidate) {
                    continue;
                }
                self.known.insert(candidate.clone());
                self.reminded.insert(candidate.clone());
                return Some(format!(
                    "Instruction file {} was not in the prompt. Read it before continuing; this reminder does not include the file body.",
                    candidate.display()
                ));
            }
            if &current == root {
                break;
            }
            directory = current.parent().map(Path::to_path_buf);
        }
        None
    }
}

fn normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn ignored(root: &Path, file: &Path) -> bool {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    let mut directory = file.parent();
    let mut files = Vec::new();
    while let Some(current) = directory {
        files.push(current.join(".gitignore"));
        if current == root {
            break;
        }
        if !current.starts_with(root) {
            break;
        }
        directory = current.parent();
    }
    files.reverse();
    let mut added = false;
    for path in files {
        if path.is_file() {
            added = true;
            let _ = builder.add(path);
        }
    }
    if !added {
        return false;
    }
    let Ok(set) = builder.build() else {
        return false;
    };
    let relative = file.strip_prefix(root).unwrap_or(file);
    if set.matched(relative, false).is_ignore() {
        return true;
    }
    let mut prefix = PathBuf::new();
    let parents = relative.parent().unwrap_or(relative);
    for component in parents.components() {
        prefix.push(component);
        if set.matched(&prefix, true).is_ignore() {
            return true;
        }
    }
    false
}

struct LiveProximity {
    roots: Vec<PathBuf>,
    proximity: InstructionProximity,
}

static PROCESS: OnceLock<Mutex<Option<LiveProximity>>> = OnceLock::new();

fn lock() -> std::sync::MutexGuard<'static, Option<LiveProximity>> {
    PROCESS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

pub fn note_loaded(roots: &[PathBuf], files: &[PathBuf]) {
    let roots: Vec<_> = roots.iter().map(|path| normalize(path)).collect();
    let mut guard = lock();
    match guard.as_mut() {
        Some(live) if live.roots == roots => live.proximity.remember(files),
        _ => {
            *guard = Some(LiveProximity {
                roots: roots.clone(),
                proximity: InstructionProximity::new(&roots, files),
            });
        }
    }
}

pub fn remind_for_tool(tool: &str, path: &Path) -> Option<String> {
    if !matches!(
        tool,
        "read_file"
            | "read"
            | "list_dir"
            | "edit_file"
            | "write_file"
            | "search_replace"
            | "hashline_edit"
    ) {
        return None;
    }
    lock().as_mut()?.proximity.after_touch(path)
}

#[cfg(test)]
mod tests {
    use super::InstructionProximity;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn a_nested_instruction_file_is_named_once_and_a_gitignored_file_is_not() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        fs::write(root.path().join("AGENTS.md"), "root rules\n").unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("AGENTS.md"), "nested rules\n").unwrap();
        fs::write(nested.join("lib.rs"), "fn answer() {}\n").unwrap();
        let secret = root.path().join("secret");
        fs::create_dir(&secret).unwrap();
        fs::write(secret.join("AGENTS.md"), "hidden\n").unwrap();
        fs::write(secret.join("lib.rs"), "fn hidden() {}\n").unwrap();
        fs::write(root.path().join(".gitignore"), "secret/\n").unwrap();

        let mut proximity =
            InstructionProximity::new(&[root_path.clone()], &[root.path().join("AGENTS.md")]);
        assert!(proximity
            .after_touch(&root.path().join("AGENTS.md"))
            .is_none());
        let note = proximity
            .after_touch(&nested.join("lib.rs"))
            .expect("nested file");
        assert!(note.contains(&nested.join("AGENTS.md").display().to_string()));
        assert!(!note.contains("nested rules"));
        assert!(proximity.after_touch(&nested.join("lib.rs")).is_none());
        assert!(proximity.after_touch(&secret.join("lib.rs")).is_none());
        let _ = PathBuf::from(note);
    }
}
