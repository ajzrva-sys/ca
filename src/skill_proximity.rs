//! After a read, list, or edit, remind once about a nearby skill directory that
//! was not in the startup set. The reminder names that directory and does not
//! include the skill body.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SKILL_PARENTS: &[&str] = &[".ca", ".agents", ".claude", ".grok", ".cursor"];
const SKILL_LEAF: &str = "skills";

/// Startup skill directories already known, plus directories already named.
#[derive(Debug, Clone)]
pub struct SkillProximity {
    root: PathBuf,
    startup: HashSet<PathBuf>,
    reminded: HashSet<PathBuf>,
}

impl SkillProximity {
    /// `root` bounds the walk. `startup` directories are silent.
    pub fn new(
        root: impl Into<PathBuf>,
        startup: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, String> {
        let root = canonicalize_dir(root.into())?;
        let mut known = HashSet::new();
        for path in startup {
            known.insert(normalize_existing(path));
        }
        Ok(Self {
            root,
            startup: known,
            reminded: HashSet::new(),
        })
    }

    pub fn after_read(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.remind(path)
    }

    pub fn after_list(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.remind(path)
    }

    pub fn after_edit(&mut self, path: &Path) -> Result<Option<String>, String> {
        self.remind(path)
    }

    fn remind(&mut self, path: &Path) -> Result<Option<String>, String> {
        let touched = fs::canonicalize(path).map_err(|err| {
            format!(
                "skill proximity could not resolve {}: {err}",
                path.display()
            )
        })?;
        if !touched.starts_with(&self.root) {
            return Err(format!(
                "{} is outside {}",
                touched.display(),
                self.root.display()
            ));
        }
        for skill_md in nearby_skill_files(&self.root, &touched) {
            if self.already_known(&skill_md) {
                continue;
            }
            self.reminded.insert(skill_md.clone());
            let dir = skill_md
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(skill_md.clone());
            return Ok(Some(format!(
                "Skill directory {} has a SKILL.md that is not in the prompt. \
                 Read it before continuing; this reminder does not include the skill body.",
                dir.display()
            )));
        }
        Ok(None)
    }

    fn already_known(&self, skill_md: &Path) -> bool {
        self.reminded.iter().any(|known| known == skill_md)
            || self
                .startup
                .iter()
                .any(|known| skill_md == known || skill_md.starts_with(known))
    }

    pub fn remember_prompted(&mut self, skills: &[PathBuf]) {
        for path in skills {
            self.startup.insert(normalize_existing(path.clone()));
        }
    }
}

struct LiveProximity {
    root: PathBuf,
    proximity: SkillProximity,
}

static PROCESS: OnceLock<Mutex<Option<LiveProximity>>> = OnceLock::new();

/// One reminder for a read, list, or edit under `root`. A later workspace root
/// rebuilds the startup set so a previous directory does not stay sticky.
pub fn remind_for_tool(root: &Path, tool: &str, path: &Path) -> Option<String> {
    let canonical = fs::canonicalize(root).ok()?;
    let mut guard = PROCESS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let rebuild = match guard.as_ref() {
        Some(live) => live.root != canonical,
        None => true,
    };
    if rebuild {
        let proximity = SkillProximity::new(&canonical, std::iter::empty()).ok()?;
        *guard = Some(LiveProximity {
            root: canonical,
            proximity,
        });
    }
    let live = guard.as_mut()?;
    let reminded = match tool {
        "read_file" | "read" => live.proximity.after_read(path),
        "list_dir" => live.proximity.after_list(path),
        "edit_file" | "write_file" | "hashline_edit" => live.proximity.after_edit(path),
        _ => return None,
    };
    reminded.ok().flatten()
}

/// Record skill files whose bodies are already in the prompt.
pub fn note_prompted_skills(root: &Path, skills: &[PathBuf]) {
    let Some(canonical) = fs::canonicalize(root).ok() else {
        return;
    };
    let mut guard = PROCESS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let rebuild = match guard.as_ref() {
        Some(live) => live.root != canonical,
        None => true,
    };
    if rebuild {
        let Some(proximity) = SkillProximity::new(&canonical, std::iter::empty()).ok() else {
            return;
        };
        *guard = Some(LiveProximity {
            root: canonical,
            proximity,
        });
    }
    if let Some(live) = guard.as_mut() {
        live.proximity.remember_prompted(skills);
    }
}

fn nearby_skill_files(root: &Path, touched: &Path) -> Vec<PathBuf> {
    let mut dir = if touched.is_dir() {
        touched.to_path_buf()
    } else {
        touched
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| touched.to_path_buf())
    };
    let mut found = Vec::new();
    loop {
        if !dir.starts_with(root) {
            break;
        }
        for parent in SKILL_PARENTS {
            let candidate = dir.join(parent).join(SKILL_LEAF);
            collect_skill_files(&candidate, &mut found);
        }
        if dir == root || !dir.pop() {
            break;
        }
    }
    found
}

fn collect_skill_files(skills_dir: &Path, found: &mut Vec<PathBuf>) {
    let direct = skills_dir.join("SKILL.md");
    if direct.is_file() {
        found.push(fs::canonicalize(&direct).unwrap_or(direct));
    }
    let Ok(entries) = fs::read_dir(skills_dir) else {
        return;
    };
    for entry in entries.flatten().take(32) {
        let skill_md = entry.path().join("SKILL.md");
        if skill_md.is_file() {
            found.push(fs::canonicalize(&skill_md).unwrap_or(skill_md));
        }
    }
}

fn canonicalize_dir(path: PathBuf) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(&path).map_err(|err| {
        format!(
            "skill search root is not a directory: {}: {err}",
            path.display()
        )
    })?;
    if !canonical.is_dir() {
        return Err(format!(
            "skill search root is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn normalize_existing(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::SkillProximity;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TREE_SEQ: AtomicU64 = AtomicU64::new(0);

    const SENTINEL: &str = "SKILL_BODY_SENTINEL_do_not_paste_9f3a";

    struct TempTree(PathBuf);

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_tree() -> Result<TempTree, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_nanos();
        let seq = TREE_SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "ca-skill-proximity-{}-{stamp}-{seq}",
            std::process::id()
        ));
        let skills = root.join(".agents").join("skills").join("demo");
        fs::create_dir_all(&skills).map_err(|err| err.to_string())?;
        fs::create_dir_all(root.join("src")).map_err(|err| err.to_string())?;
        fs::write(skills.join("SKILL.md"), SENTINEL).map_err(|err| err.to_string())?;
        fs::write(root.join("src").join("lib.rs"), "fn lib() {}\n")
            .map_err(|err| err.to_string())?;
        Ok(TempTree(root))
    }

    #[test]
    fn new_skill_dir_is_named_once_without_its_body() -> Result<(), String> {
        let tree = temp_tree()?;
        let skill_dir = fs::canonicalize(tree.0.join(".agents").join("skills"))
            .map_err(|err| err.to_string())?;
        let file = tree.0.join("src").join("lib.rs");
        let mut proximity = SkillProximity::new(&tree.0, Vec::<PathBuf>::new())?;
        let reminder = proximity
            .after_list(&file)?
            .expect("skill dir was not in the startup set");
        assert!(reminder.contains(&skill_dir.display().to_string()));
        assert!(!reminder.contains(SENTINEL));
        assert!(proximity.after_read(&file)?.is_none());
        assert!(proximity.after_edit(&file)?.is_none());
        Ok(())
    }

    #[test]
    fn startup_skill_dir_is_silent() -> Result<(), String> {
        let tree = temp_tree()?;
        let skill_dir = fs::canonicalize(tree.0.join(".agents").join("skills"))
            .map_err(|err| err.to_string())?;
        let file = tree.0.join("src").join("lib.rs");
        let mut proximity = SkillProximity::new(&tree.0, vec![skill_dir])?;
        assert!(proximity.after_read(&file)?.is_none());
        assert!(proximity.after_edit(&file)?.is_none());
        Ok(())
    }

    #[test]
    fn a_prompted_skill_stays_quiet_and_a_ca_skill_is_named() -> Result<(), String> {
        let tree = temp_tree()?;
        let prompted = fs::canonicalize(tree.0.join(".agents/skills/demo/SKILL.md"))
            .map_err(|err| err.to_string())?;
        let ca_skill = tree.0.join(".ca/skills/local");
        fs::create_dir_all(&ca_skill).map_err(|err| err.to_string())?;
        fs::write(ca_skill.join("SKILL.md"), SENTINEL).map_err(|err| err.to_string())?;
        let file = tree.0.join("src").join("lib.rs");
        let mut proximity = SkillProximity::new(&tree.0, Vec::<PathBuf>::new())?;
        proximity.remember_prompted(&[prompted]);
        let reminder = proximity
            .after_read(&file)?
            .expect("the .ca skill was not in the prompt");
        assert!(reminder.contains(".ca"));
        assert!(!reminder.contains(".agents"));
        assert!(!reminder.contains(SENTINEL));
        assert!(proximity.after_read(&file)?.is_none());
        Ok(())
    }
}
