//! Repository and orchestrator path layout.
//!
//! Owns construction of stable filesystem paths used by the app conductor and
//! subsystems. This module is intentionally side-effect light: it canonicalizes
//! the repository root and derives paths, but does not create files, read config,
//! mutate state, or perform Git operations.
//!
//! Read when changing where `.orchestrator` artifacts or root control documents
//! live. Do not read for queue parsing, verification command behavior, role
//! prompts, or terminal rendering.

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Paths {
    // All orchestrator-owned artifacts live under `.orchestrator` except the
    // human-facing control docs in the repository root/docs tree.
    pub(crate) root: PathBuf,
    pub(crate) orch: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) prompts: PathBuf,
    pub(crate) schemas: PathBuf,
    pub(crate) tasks: PathBuf,
    pub(crate) runs: PathBuf,
    pub(crate) recipes: PathBuf,
    pub(crate) observations: PathBuf,
    pub(crate) evidence: PathBuf,
    pub(crate) events: PathBuf,
    pub(crate) cost: PathBuf,
    pub(crate) lock: PathBuf,
    pub(crate) roadmap: PathBuf,
    pub(crate) next_steps: PathBuf,
    pub(crate) next_steps_non_ai: PathBuf,
    pub(crate) agents: PathBuf,
}

impl Paths {
    pub(crate) fn new(repo: &Path) -> Result<Self> {
        let root = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
        let orch = root.join(".orchestrator");
        Ok(Self {
            config: orch.join("config.toml"),
            state: orch.join("state.json"),
            prompts: orch.join("prompts"),
            schemas: orch.join("schemas"),
            tasks: orch.join("tasks"),
            runs: orch.join("runs"),
            recipes: orch.join("recipes"),
            observations: orch.join("observations"),
            evidence: orch.join("evidence"),
            events: orch.join("events.jsonl"),
            cost: orch.join("cost.jsonl"),
            lock: orch.join("codex-team.lock"),
            roadmap: root.join("ROADMAP.md"),
            next_steps: root.join("NEXT_STEPS.md"),
            next_steps_non_ai: root.join("NEXT_STEPS_non-ai.md"),
            agents: root.join("AGENTS.md"),
            root,
            orch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codex-team-paths-{name}-{nonce}"))
    }

    #[cfg(unix)]
    #[test]
    fn paths_new_canonicalizes_symlinked_repo_root() -> Result<()> {
        let root = temp_root("symlink-root");
        let actual = root.join("actual");
        let link = root.join("link");
        fs::create_dir_all(&actual)?;
        std::os::unix::fs::symlink(&actual, &link)?;

        let paths = Paths::new(&link)?;

        assert_eq!(paths.root, fs::canonicalize(&actual)?);
        assert_eq!(paths.orch, paths.root.join(".orchestrator"));
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn paths_new_keeps_prefix_sibling_roots_distinct() -> Result<()> {
        let root = temp_root("prefix-boundary");
        let repo = root.join("repo");
        let sibling = root.join("repo-other");
        fs::create_dir_all(&repo)?;
        fs::create_dir_all(&sibling)?;

        let paths = Paths::new(&repo)?;

        assert_eq!(paths.root, fs::canonicalize(&repo)?);
        assert_ne!(paths.root, fs::canonicalize(&sibling)?);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
