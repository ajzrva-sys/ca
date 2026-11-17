//! In-process git status and diff used when native git metadata cannot be read.
//! This module does not spawn git.

use std::path::{Path, PathBuf};

const STATUS_BYTE_CAP: usize = 64 * 1024;
const DIFF_BYTE_CAP: usize = 8 * 1024;
const MAX_SCAN_FILES: usize = 1024;

/// Hard ceiling for status workers.
pub const MAX_STATUS_THREADS: usize = 8;

/// Soft `RLIMIT_NPROC` below this uses `TIGHT_THREADS`.
pub const NPROC_TIGHT: u64 = 32;

/// Soft `RLIMIT_NPROC` below this uses a single worker.
pub const NPROC_TIGHTER: u64 = 16;

pub const TIGHT_THREADS: usize = 2;
pub const TIGHTER_THREADS: usize = 1;

/// Thread count for a status scan. Never above 8, never below 1.
/// A soft nproc under 32 uses 2; under 16 uses 1. `None` is the ceiling.
pub fn soft_nproc() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &mut limit) } == 0 {
            return Some(limit.rlim_cur);
        }
    }
    None
}

pub fn thread_cap(soft_nproc: Option<u64>) -> usize {
    let Some(limit) = soft_nproc else {
        return MAX_STATUS_THREADS;
    };
    if limit < NPROC_TIGHTER {
        TIGHTER_THREADS
    } else if limit < NPROC_TIGHT {
        TIGHT_THREADS
    } else {
        MAX_STATUS_THREADS
    }
}

/// Error text for a failed scan. `spawn_failed` selects the reason. Does not panic.
pub fn scan_failure_is_reported(spawn_failed: bool) -> String {
    let reason = if spawn_failed {
        "spawn failed"
    } else {
        "scan failed"
    };
    format!("git status error: {reason}")
}

/// Join status lines, stopping so the result stays within `max_bytes`.
pub fn bounded_status(entries: &[String], max_bytes: usize) -> String {
    let mut out = String::new();
    for entry in entries {
        let line = format!("{entry}\n");
        let room = max_bytes.saturating_sub(out.len());
        if room == 0 {
            break;
        }
        if line.len() <= room {
            out.push_str(&line);
            continue;
        }
        out.push_str(byte_prefix(&line, room));
        break;
    }
    out
}

/// A readable `.git/HEAD` is a normal repository. An unreadable HEAD is not.
pub fn head_readable(root: &Path) -> bool {
    std::fs::File::open(root.join(".git").join("HEAD")).is_ok()
}

/// `.git` is present as a directory but `HEAD` cannot be read.
pub fn metadata_down(root: &Path) -> bool {
    let git = root.join(".git");
    git.is_dir() && !git.join("HEAD").is_file()
}

/// Worktree paths as `??` lines. A worker that cannot spawn returns the spawn failure.
pub fn in_process_status(root: &Path) -> Result<String, String> {
    let threads = thread_cap(soft_nproc());
    match collect_untracked(root, threads) {
        Ok(entries) => Ok(bounded_status(&entries, STATUS_BYTE_CAP)),
        Err(spawn_failed) => Err(scan_failure_is_reported(spawn_failed)),
    }
}

/// File body as a bounded diff when git metadata cannot supply one.
pub fn in_process_diff(root: &Path, relative: &Path) -> Result<String, String> {
    let full = root.join(relative);
    if let (Ok(root), Ok(file)) = (root.canonicalize(), full.canonicalize()) {
        if !file.starts_with(&root) {
            return Err(scan_failure_is_reported(false));
        }
    }
    let bytes = std::fs::read(&full).map_err(|_| scan_failure_is_reported(false))?;
    let text = String::from_utf8_lossy(&bytes);
    let rel = relative.display().to_string();
    let mut lines = vec![format!("diff --git a/{rel} b/{rel}")];
    for line in text.lines().take(400) {
        lines.push(format!("+{line}"));
    }
    Ok(bounded_status(&lines, DIFF_BYTE_CAP))
}

fn collect_untracked(root: &Path, threads: usize) -> Result<Vec<String>, bool> {
    let mut files = Vec::new();
    walk_files(root, &mut files);
    files.sort();
    files.truncate(MAX_SCAN_FILES);
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let threads = threads.clamp(1, MAX_STATUS_THREADS).min(files.len());
    let mut chunks: Vec<Vec<PathBuf>> = (0..threads).map(|_| Vec::new()).collect();
    for (index, file) in files.into_iter().enumerate() {
        chunks[index % threads].push(file);
    }
    let root = root.to_path_buf();
    let mut started = Vec::new();
    for chunk in chunks {
        let root = root.clone();
        match std::thread::Builder::new().spawn(move || label_untracked(&root, &chunk)) {
            Ok(handle) => started.push(handle),
            Err(_) => {
                for handle in started {
                    let _ = handle.join();
                }
                return Err(true);
            }
        }
    }
    let mut entries = Vec::new();
    for handle in started {
        match handle.join() {
            Ok(mut part) => entries.append(&mut part),
            Err(_) => return Err(false),
        }
    }
    entries.sort();
    Ok(entries)
}

fn label_untracked(root: &Path, files: &[PathBuf]) -> Vec<String> {
    files
        .iter()
        .map(|path| {
            let rel = path.strip_prefix(root).unwrap_or(path);
            format!("?? {}", rel.display())
        })
        .collect()
}

fn walk_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if files.len() >= MAX_SCAN_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_SCAN_FILES {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some(".git") {
                continue;
            }
            walk_files(&path, files);
        } else if kind.is_file() {
            files.push(path);
        }
    }
}

fn byte_prefix(text: &str, max_bytes: usize) -> &str {
    if max_bytes >= text.len() {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_cap_follows_the_nproc_ladder() {
        assert_eq!(thread_cap(None), MAX_STATUS_THREADS);
        assert_eq!(thread_cap(Some(u64::MAX)), MAX_STATUS_THREADS);
        assert_eq!(thread_cap(Some(NPROC_TIGHT)), MAX_STATUS_THREADS);
        assert_eq!(thread_cap(Some(NPROC_TIGHT - 1)), TIGHT_THREADS);
        assert_eq!(thread_cap(Some(NPROC_TIGHTER)), TIGHT_THREADS);
        assert_eq!(thread_cap(Some(NPROC_TIGHTER - 1)), TIGHTER_THREADS);
        assert_eq!(thread_cap(Some(0)), TIGHTER_THREADS);
        assert!(thread_cap(Some(1)) >= 1);
        assert!(thread_cap(Some(10_000)) <= MAX_STATUS_THREADS);
    }

    #[test]
    fn bounded_status_stays_inside_the_byte_cap() {
        let entries = vec!["abcdef".to_string(), "ghijkl".to_string()];
        let full = bounded_status(&entries, 10_000);
        let bounded = bounded_status(&entries, 8);
        assert!(bounded.len() <= 8, "{}", bounded.len());
        assert!(full.starts_with(&bounded));
        assert_eq!(bounded_status(&entries, 0), "");
        let reported = scan_failure_is_reported(true);
        assert!(reported.contains("spawn"));
        assert!(scan_failure_is_reported(false).contains("scan"));
    }
}
