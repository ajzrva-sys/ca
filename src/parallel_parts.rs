//! One worktree per independent part and one host three-way merge.
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartChild {
    pub worktree: PathBuf,
    pub running: bool,
    pub agent: Option<String>,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParallelStart {
    pub base: String,
    pub children: Vec<PartChild>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeBatch {
    pub parent: PathBuf,
    pub files: Vec<MergedFile>,
    pub committed: bool,
    pub loose_ends: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedFile {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

pub fn start_parts(repo: &Path, file_lists: &[Vec<String>]) -> Result<ParallelStart, String> {
    if file_lists.len() < 2 {
        return Err("a parallel run needs at least two parts".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for list in file_lists {
        for file in list {
            if !seen.insert(file.as_str()) {
                return Err(format!("overlapping file list: {file}"));
            }
        }
    }
    let links = load_worktree_links(repo)?;
    let base = git(repo, &["rev-parse", "HEAD"])?;
    let mut children = Vec::new();
    for (index, _) in file_lists.iter().enumerate() {
        let path = repo.join(format!(".parallel-part-{index}"));
        if path.exists() {
            return Err("part worktree already exists".into());
        }
        let path_arg = path.to_str().ok_or("worktree path")?.to_string();
        git(repo, &["worktree", "add", "--detach", &path_arg, &base])?;
        let child_head = git(&path, &["rev-parse", "HEAD"])?;
        if child_head != base {
            return Err("worktree is not at the parent HEAD".into());
        }
        apply_worktree_links(repo, &path, &links)?;
        children.push(PartChild {
            worktree: path,
            running: true,
            agent: None,
            port: part_port(index),
        });
    }
    if !children.iter().all(|child| child.running) {
        return Err("children were not started together".into());
    }
    Ok(ParallelStart { base, children })
}

pub fn merge_parts(
    parent: &Path,
    base: &str,
    parts: &[(String, PathBuf)],
) -> Result<MergeBatch, String> {
    if parts.is_empty() {
        return Ok(MergeBatch {
            parent: parent.to_path_buf(),
            files: Vec::new(),
            committed: false,
            loose_ends: Vec::new(),
        });
    }
    let mut names = std::collections::BTreeSet::new();
    for (_, worktree) in parts {
        let status = git(worktree, &["diff", "--name-only", "HEAD"])?;
        for line in status.lines() {
            let name = line.trim();
            if !name.is_empty() {
                names.insert(name.to_string());
            }
        }
    }
    let parent_status = git(parent, &["diff", "--name-only", "HEAD"])?;
    for line in parent_status.lines() {
        let name = line.trim();
        if !name.is_empty() {
            names.insert(name.to_string());
        }
    }
    let mut files = Vec::new();
    for name in names {
        let spec = format!("{base}:{name}");
        let base_text = git_stdout(parent, &["show", &spec]).unwrap_or_default();
        let parent_text = std::fs::read_to_string(parent.join(&name)).unwrap_or_default();
        let mut part_texts = Vec::new();
        for (label, worktree) in parts {
            let text = std::fs::read_to_string(worktree.join(&name)).unwrap_or_default();
            if text != base_text {
                part_texts.push((label.clone(), text));
            }
        }
        let parent_edited = parent_text != base_text;
        let after = if part_texts.is_empty() {
            parent_text.clone()
        } else if !parent_edited && part_texts.len() == 1 {
            part_texts[0].1.clone()
        } else {
            conflict_text(&base_text, &parent_text, &part_texts)
        };
        if after != parent_text {
            std::fs::write(parent.join(&name), &after).map_err(|error| error.to_string())?;
        }
        files.push(MergedFile {
            path: parent.join(&name),
            before: parent_text,
            after,
        });
    }
    let mut loose_ends = Vec::new();
    for (_, worktree) in parts {
        for line in read_loose_ends(worktree) {
            if loose_ends.len() == 16 {
                break;
            }
            loose_ends.push(line);
        }
    }
    let batch = MergeBatch {
        parent: parent.to_path_buf(),
        files,
        committed: false,
        loose_ends,
    };
    if let Ok(mut slot) = last_merge().lock() {
        *slot = Some(batch.clone());
    }
    Ok(batch)
}

fn last_merge() -> &'static std::sync::Mutex<Option<MergeBatch>> {
    static LAST: std::sync::OnceLock<std::sync::Mutex<Option<MergeBatch>>> =
        std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn stored_merge() -> Option<MergeBatch> {
    last_merge()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

pub fn restore_before(batch: &MergeBatch) -> Result<(), String> {
    for file in &batch.files {
        std::fs::write(&file.path, &file.before).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn start_one(repo: &Path, files: &[String]) -> Result<PartChild, String> {
    if files.is_empty() {
        return Err("a parallel part needs a file list".into());
    }
    let links = load_worktree_links(repo)?;
    let base = git(repo, &["rev-parse", "HEAD"])?;
    let path = repo.join(".parallel-part-live");
    if path.exists() {
        return Err("part worktree already exists".into());
    }
    let path_arg = path.to_str().ok_or("worktree path")?.to_string();
    git(repo, &["worktree", "add", "--detach", &path_arg, &base])?;
    let child_head = git(&path, &["rev-parse", "HEAD"])?;
    if child_head != base {
        return Err("worktree is not at the parent HEAD".into());
    }
    apply_worktree_links(repo, &path, &links)?;
    let child = PartChild {
        worktree: path,
        running: true,
        agent: None,
        port: part_port(0),
    };
    register_running(child.clone());
    Ok(child)
}

pub fn register_running(child: PartChild) {
    recorded()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(child);
}

pub fn any_running() -> bool {
    recorded()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .any(|child| child.running)
}

pub fn attach_agent(worktree: &Path, agent: &str) {
    for child in recorded()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter_mut()
    {
        if child.worktree == worktree {
            child.agent = Some(agent.to_string());
        }
    }
}

#[derive(Clone)]
struct PendingMerge {
    parent: PathBuf,
    base: String,
    parts: Vec<(String, PathBuf)>,
}

fn pending_merges() -> &'static std::sync::Mutex<Vec<PendingMerge>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<Vec<PendingMerge>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn remember_pending(parent: &Path, base: &str, parts: Vec<(String, PathBuf)>) {
    pending_merges()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push(PendingMerge {
            parent: parent.to_path_buf(),
            base: base.to_string(),
            parts,
        });
}

pub fn mark_finished(worktree: &Path) {
    {
        let mut recorded = recorded()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for child in recorded.iter_mut() {
            if child.worktree == worktree {
                child.running = false;
            }
        }
    }
    let ready = {
        let recorded = recorded()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut jobs = pending_merges()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut ready = Vec::new();
        jobs.retain(|job| {
            let live = job.parts.iter().any(|(_, path)| {
                recorded
                    .iter()
                    .any(|child| child.worktree == *path && child.running)
            });
            if live {
                true
            } else {
                ready.push(job.clone());
                false
            }
        });
        ready
    };
    for job in ready {
        let _merged = merge_parts(&job.parent, &job.base, &job.parts);
    }
}

/// Cleanup calls this before it removes a worktree. A live part is refused.
pub fn discard_recorded(id: &str) -> Result<(), String> {
    let recorded = recorded()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(child) = recorded
        .iter()
        .find(|child| child.worktree.ends_with(id) || child.worktree.display().to_string() == id)
    {
        discard_part(child)?;
    }
    Ok(())
}

pub fn discard_part(child: &PartChild) -> Result<(), String> {
    if child.running {
        return Err("Discard of a running child is refused".into());
    }
    Ok(())
}

fn recorded() -> &'static std::sync::Mutex<Vec<PartChild>> {
    static RECORDED: std::sync::OnceLock<std::sync::Mutex<Vec<PartChild>>> =
        std::sync::OnceLock::new();
    RECORDED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

pub fn restore_after(batch: &MergeBatch) -> Result<(), String> {
    for file in &batch.files {
        std::fs::write(&file.path, &file.after).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn conflict_text(base: &str, parent: &str, parts: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    let parent_lines: Vec<&str> = parent.lines().collect();
    let base_lines: Vec<&str> = base.lines().collect();
    let width = parent_lines.len().max(base_lines.len()).max(
        parts
            .iter()
            .map(|(_, text)| text.lines().count())
            .max()
            .unwrap_or(0),
    );
    for index in 0..width {
        let parent_line = parent_lines.get(index).copied().unwrap_or("");
        let base_line = base_lines.get(index).copied().unwrap_or("");
        let changed: Vec<_> = parts
            .iter()
            .filter_map(|(label, text)| {
                let line = text.lines().nth(index).unwrap_or("");
                (line != base_line).then_some((label.as_str(), line))
            })
            .collect();
        if changed.is_empty() {
            lines.push(parent_line.to_string());
            continue;
        }
        if !parent_line.is_empty() && parent_line != base_line {
            lines.push(format!("<<<<<<< parent\n{parent_line}"));
        }
        for (label, line) in changed {
            lines.push(format!("<<<<<<< {label}\n{line}"));
        }
        lines.push(">>>>>>> merge".to_string());
    }
    if lines.is_empty() {
        parent.to_string()
    } else {
        lines.join("\n") + "\n"
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    Ok(git_stdout(cwd, args)?.trim().to_string())
}

pub fn part_port(index: usize) -> u16 {
    20_000 + u16::try_from(index).unwrap_or(u16::MAX).saturating_mul(10)
}

pub fn port_line(port: u16) -> String {
    format!("\nCA_PORT={port}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkKind {
    Link,
    Copy,
}

fn load_worktree_links(repo: &Path) -> Result<Vec<(String, LinkKind)>, String> {
    let path = repo.join(".ca/worktree-links.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let map: std::collections::BTreeMap<String, String> =
        toml::from_str(&text).map_err(|error| error.to_string())?;
    let mut links = Vec::new();
    for (name, kind) in map {
        reject_link_name(&name)?;
        let kind = match kind.as_str() {
            "link" => LinkKind::Link,
            "copy" => LinkKind::Copy,
            _ => return Err(format!("unsupported worktree link kind for {name}")),
        };
        links.push((name, kind));
    }
    Ok(links)
}

fn reject_link_name(name: &str) -> Result<(), String> {
    let lower = name.to_ascii_lowercase();
    if lower == ".env"
        || lower.starts_with(".env.")
        || lower == "secrets"
        || lower.contains("credential")
        || lower.contains("secret")
    {
        return Err(format!("worktree link rejected: {name}"));
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!("worktree link rejected: {name}"));
    }
    Ok(())
}

fn apply_worktree_links(
    repo: &Path,
    worktree: &Path,
    links: &[(String, LinkKind)],
) -> Result<(), String> {
    for (name, kind) in links {
        let source = repo.join(name);
        let dest = worktree.join(name);
        if dest.exists() || std::fs::symlink_metadata(&dest).is_ok() {
            continue;
        }
        if !source.exists() {
            continue;
        }
        match kind {
            LinkKind::Link => {
                std::os::unix::fs::symlink(&source, &dest).map_err(|error| error.to_string())?;
            }
            LinkKind::Copy => {
                let status = std::process::Command::new("cp")
                    .args(["-cR", &source.to_string_lossy(), &dest.to_string_lossy()])
                    .status()
                    .map_err(|error| error.to_string())?;
                if !status.success() {
                    return Err(format!("worktree copy failed for {name}"));
                }
            }
        }
    }
    Ok(())
}

fn read_loose_ends(worktree: &Path) -> Vec<String> {
    let path = worktree.join(".ca/loose-ends.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(16)
        .map(str::to_string)
        .collect()
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]).unwrap();
        git(dir.path(), &["config", "user.email", "t@example.com"]).unwrap();
        git(dir.path(), &["config", "user.name", "t"]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "base-a\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "base-b\n").unwrap();
        git(dir.path(), &["add", "."]).unwrap();
        git(dir.path(), &["commit", "-m", "init"]).unwrap();
        dir
    }

    #[test]
    fn two_parts_share_head_and_overlap_creates_no_worktree() {
        let dir = repo();
        let started =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap();
        assert_eq!(started.children.len(), 2);
        assert!(started.children.iter().all(|child| child.running));
        assert_eq!(
            git(&started.children[0].worktree, &["rev-parse", "HEAD"]).unwrap(),
            git(&started.children[1].worktree, &["rev-parse", "HEAD"]).unwrap()
        );
        let before = git(dir.path(), &["worktree", "list"]).unwrap();
        let err =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["a.txt".into()]]).unwrap_err();
        assert!(err.contains("overlapping"));
        assert_eq!(git(dir.path(), &["worktree", "list"]).unwrap(), before);
        let _ = started;
    }

    #[test]
    fn merge_lands_disjoint_edits_and_keeps_both_sides_of_a_conflict() {
        let dir = repo();
        std::fs::write(dir.path().join("c.txt"), "base-c\n").unwrap();
        git(dir.path(), &["add", "c.txt"]).unwrap();
        git(dir.path(), &["commit", "-m", "c"]).unwrap();
        let started =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap();
        std::fs::write(started.children[0].worktree.join("a.txt"), "part-a\n").unwrap();
        std::fs::write(started.children[0].worktree.join("b.txt"), "from-part-1\n").unwrap();
        std::fs::write(started.children[1].worktree.join("b.txt"), "part-b\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "parent-b\n").unwrap();
        std::fs::write(dir.path().join("c.txt"), "parent-only\n").unwrap();
        let parts = vec![
            ("part-1".into(), started.children[0].worktree.clone()),
            ("part-2".into(), started.children[1].worktree.clone()),
        ];
        let mut tracker = crate::automatic_review::ReviewTracker::default();
        register_running(started.children[0].clone());
        let batch = merge_parts(dir.path(), &started.base, &parts).unwrap();
        assert!(!batch.committed);
        let refused = tracker.undo_merge().unwrap();
        assert!(refused.contains("running"));
        assert!(std::fs::read_to_string(dir.path().join("b.txt"))
            .unwrap()
            .contains("<<<<<<<"));
        mark_finished(&started.children[0].worktree);
        mark_finished(&started.children[1].worktree);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "part-a\n"
        );
        let conflict = std::fs::read_to_string(dir.path().join("b.txt")).unwrap();
        assert!(conflict.contains("<<<<<<< parent"));
        assert!(conflict.contains("parent-b"));
        assert!(conflict.contains("<<<<<<< part-1"));
        assert!(conflict.contains("from-part-1"));
        assert!(conflict.contains("<<<<<<< part-2"));
        assert!(conflict.contains("part-b"));
        let parent_only = std::fs::read_to_string(dir.path().join("c.txt")).unwrap();
        assert_eq!(parent_only, "parent-only\n");
        assert!(!parent_only.contains("<<<<<<<"));
        let worktrees_exist = started.children.iter().all(|child| child.worktree.exists());
        assert_eq!(
            tracker.undo_merge().as_deref(),
            Some("Restored the parent bytes from before the merge")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "parent-b\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "base-a\n"
        );
        assert_eq!(
            tracker.redo_merge().as_deref(),
            Some("Restored the merged bytes")
        );
        assert!(std::fs::read_to_string(dir.path().join("b.txt"))
            .unwrap()
            .contains("parent-b"));
        assert!(worktrees_exist);
        assert!(started.children[0].worktree.exists());
        assert!(discard_part(&started.children[0])
            .unwrap_err()
            .contains("running"));
    }

    #[tokio::test]
    async fn cleanup_refuses_a_running_parallel_child() {
        let dir = repo();
        let started =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap();
        register_running(started.children[0].clone());
        let store = crate::worktree::WorktreeStore::new(dir.path().join("store"), true);
        let error = store
            .cleanup(&started.children[0].worktree.display().to_string(), None)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("Discard of a running child is refused"));
        mark_finished(&started.children[0].worktree);
        mark_finished(&started.children[1].worktree);
    }

    #[test]
    fn worktree_link_port_and_loose_ends() {
        let dir = repo();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/marker"), "shared").unwrap();
        let absent =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap();
        assert!(!absent.children[0].worktree.join("node_modules").exists());
        println!(
            "ports={} {} lines={:?} {:?}",
            absent.children[0].port,
            absent.children[1].port,
            port_line(absent.children[0].port),
            port_line(absent.children[1].port)
        );
        assert_eq!(absent.children[0].port, 20_000);
        assert_eq!(absent.children[1].port, 20_010);
        assert_eq!(port_line(part_port(0)), "\nCA_PORT=20000");
        assert_eq!(port_line(part_port(1)), "\nCA_PORT=20010");
        for child in &absent.children {
            mark_finished(&child.worktree);
            git(
                dir.path(),
                &[
                    "worktree",
                    "remove",
                    "--force",
                    &child.worktree.to_string_lossy(),
                ],
            )
            .unwrap();
        }
        std::fs::create_dir_all(dir.path().join(".ca")).unwrap();
        std::fs::write(
            dir.path().join(".ca/worktree-links.toml"),
            ".env = \"link\"\n",
        )
        .unwrap();
        let rejected =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap_err();
        assert!(rejected.contains(".env"));
        std::fs::write(
            dir.path().join(".ca/worktree-links.toml"),
            "node_modules = \"link\"\n",
        )
        .unwrap();
        let linked =
            start_parts(dir.path(), &[vec!["a.txt".into()], vec!["b.txt".into()]]).unwrap();
        let link = linked.children[0].worktree.join("node_modules");
        let kind = std::fs::symlink_metadata(&link).unwrap().file_type();
        println!("node_modules-symlink={}", kind.is_symlink());
        assert!(kind.is_symlink());
        assert_eq!(
            std::fs::read_to_string(link.join("marker")).unwrap(),
            "shared"
        );
        std::fs::create_dir_all(linked.children[0].worktree.join(".ca")).unwrap();
        std::fs::write(
            linked.children[0].worktree.join(".ca/loose-ends.txt"),
            "remove debug flag\n",
        )
        .unwrap();
        std::fs::write(linked.children[0].worktree.join("a.txt"), "part-a\n").unwrap();
        let parts = vec![
            ("part-1".into(), linked.children[0].worktree.clone()),
            ("part-2".into(), linked.children[1].worktree.clone()),
        ];
        let batch = merge_parts(dir.path(), &linked.base, &parts).unwrap();
        println!(
            "loose={:?} merged-a={}",
            batch.loose_ends,
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap()
        );
        assert_eq!(batch.loose_ends, vec!["remove debug flag".to_string()]);
        assert_eq!(
            std::fs::read_to_string(linked.children[0].worktree.join(".ca/loose-ends.txt"))
                .unwrap(),
            "remove debug flag\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "part-a\n"
        );
        assert_eq!(linked.children[0].port, part_port(0));
        assert_eq!(linked.children[1].port, part_port(1));
    }
}
