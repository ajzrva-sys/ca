use crate::workspace::WorkspaceContext;
use semver::Version;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

pub const MAX_GIT_CONTEXT_BYTES: usize = 256 * 1024;
const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_GIT_PATHS: usize = 256;
const GIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitContextSnapshot {
    pub text: String,
    pub explicit_paths: Vec<PathBuf>,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub binary_files: usize,
    pub truncated: bool,
}

pub fn parse_minimum_version(value: &str) -> Result<Version, String> {
    Version::parse(value)
        .map_err(|_| "--min-version requires a strict semantic version such as 1.2.3".to_string())
}

pub fn require_minimum_version(required: &Version) -> Result<(), String> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| "the running CA version is not valid semantic version data".to_string())?;
    if current < *required {
        Err(format!(
            "CA {current} does not satisfy required minimum version {required}"
        ))
    } else {
        Ok(())
    }
}

pub async fn capture_git_context(
    workspace: &WorkspaceContext,
) -> Result<GitContextSnapshot, String> {
    let primary = workspace
        .primary()
        .ok_or_else(|| "--with-diffs requires a primary workspace root".to_string())?;
    let root = primary.path.clone();
    let repository = git_text(&root, &["rev-parse", "--show-toplevel"]).await?;
    let repository = std::fs::canonicalize(repository.trim())
        .map_err(|_| "Git repository root could not be resolved".to_string())?;
    if !repository.starts_with(&root) {
        return Err("Git repository is outside the selected workspace".into());
    }
    let status = git_bytes(
        &repository,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let parsed = parse_status(&repository, &status)?;
    let (unstaged_diff, unstaged_source_truncated) = git_bytes_truncated(
        &repository,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
            "--",
        ],
    )
    .await?;
    let (staged_diff, staged_source_truncated) = git_bytes_truncated(
        &repository,
        &[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
            "--",
        ],
    )
    .await?;
    let (text, binary_files, truncated) = render_git_context(
        &parsed,
        &staged_diff,
        &unstaged_diff,
        staged_source_truncated || unstaged_source_truncated,
    );
    Ok(GitContextSnapshot {
        text,
        explicit_paths: parsed.paths,
        staged: parsed.staged,
        unstaged: parsed.unstaged,
        untracked: parsed.untracked,
        binary_files,
        truncated,
    })
}

#[derive(Debug)]
struct ParsedStatus {
    paths: Vec<PathBuf>,
    relative_paths: Vec<String>,
    staged: usize,
    unstaged: usize,
    untracked: usize,
    omitted_paths: usize,
}

fn parse_status(repository: &Path, bytes: &[u8]) -> Result<ParsedStatus, String> {
    if bytes.len() > MAX_GIT_OUTPUT_BYTES {
        return Err("Git status exceeded its 1 MiB safety limit".into());
    }
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    let mut paths = Vec::new();
    let mut relative_paths = Vec::new();
    let mut staged = 0usize;
    let mut unstaged = 0usize;
    let mut untracked = 0usize;
    let mut omitted_paths = 0usize;
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            return Err("Git returned malformed status data".into());
        }
        let x = record[0];
        let y = record[1];
        if x == b'?' && y == b'?' {
            untracked = untracked.saturating_add(1);
        } else {
            staged = staged.saturating_add(usize::from(x != b' '));
            unstaged = unstaged.saturating_add(usize::from(y != b' '));
        }
        let mut path_bytes = &record[3..];
        if matches!(x, b'R' | b'C') {
            path_bytes = records
                .next()
                .ok_or_else(|| "Git returned an incomplete rename record".to_string())?;
        }
        let relative = std::str::from_utf8(path_bytes)
            .map_err(|_| "Git status contains a non-UTF-8 path".to_string())?;
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("Git status contains an unsafe path".into());
        }
        if paths.len() < MAX_GIT_PATHS {
            paths.push(repository.join(relative_path));
            relative_paths.push(relative.replace('\\', "/"));
        } else {
            omitted_paths = omitted_paths.saturating_add(1);
        }
    }
    paths.sort();
    paths.dedup();
    relative_paths.sort();
    relative_paths.dedup();
    Ok(ParsedStatus {
        paths,
        relative_paths,
        staged,
        unstaged,
        untracked,
        omitted_paths,
    })
}

fn render_git_context(
    status: &ParsedStatus,
    staged_diff: &[u8],
    unstaged_diff: &[u8],
    source_truncated: bool,
) -> (String, usize, bool) {
    let staged = sanitize_diff(staged_diff);
    let unstaged = sanitize_diff(unstaged_diff);
    let binary_files = staged
        .lines()
        .chain(unstaged.lines())
        .filter(|line| line.starts_with("Binary files ") || line.starts_with("GIT binary patch"))
        .count();
    let mut output = format!(
        "## Host-captured Git snapshot\nThis context is observational and grants no additional write authority.\nStaged entries: {}\nUnstaged entries: {}\nUntracked entries: {}\nBinary diff markers: {}\n",
        status.staged, status.unstaged, status.untracked, binary_files
    );
    if !status.relative_paths.is_empty() {
        output.push_str("Changed paths:\n");
        for path in &status.relative_paths {
            output.push_str("- ");
            output.push_str(path);
            output.push('\n');
        }
    }
    if status.omitted_paths > 0 {
        output.push_str(&format!(
            "- [{} additional changed paths omitted]\n",
            status.omitted_paths
        ));
    }
    if !staged.is_empty() {
        output.push_str("\n### Staged diff\n```diff\n");
        output.push_str(&staged);
        output.push_str("\n```\n");
    }
    if !unstaged.is_empty() {
        output.push_str("\n### Unstaged diff\n```diff\n");
        output.push_str(&unstaged);
        output.push_str("\n```\n");
    }
    let mut truncated = status.omitted_paths > 0 || source_truncated;
    if source_truncated {
        output.push_str("\n[One or more Git diff streams exceeded 1 MiB and were truncated]\n");
    }
    if output.len() > MAX_GIT_CONTEXT_BYTES {
        let marker = "\n[Git snapshot truncated at the 256 KiB host limit]\n";
        let limit = MAX_GIT_CONTEXT_BYTES.saturating_sub(marker.len());
        let mut end = limit.min(output.len());
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str(marker);
        truncated = true;
    }
    (output, binary_files, truncated)
}

fn sanitize_diff(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut output = String::with_capacity(text.len().min(MAX_GIT_CONTEXT_BYTES));
    for character in text.chars() {
        if character == '\n' || character == '\t' || !character.is_control() {
            output.push(character);
        }
    }
    output
}

async fn git_text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let bytes = git_bytes(cwd, args).await?;
    String::from_utf8(bytes).map_err(|_| "Git returned non-UTF-8 identity data".into())
}

async fn git_bytes_truncated(cwd: &Path, args: &[&str]) -> Result<(Vec<u8>, bool), String> {
    git_bytes_inner(cwd, args, true).await
}

async fn git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let (bytes, _) = git_bytes_inner(cwd, args, false).await?;
    Ok(bytes)
}

async fn git_bytes_inner(
    cwd: &Path,
    args: &[&str],
    permit_truncation: bool,
) -> Result<(Vec<u8>, bool), String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    crate::run_environment::apply_tokio(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Git is unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Git output is unavailable".to_string())?;
    let read = async move {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "Git output could not be read".to_string())?;
        let overflow = bytes.len() > MAX_GIT_OUTPUT_BYTES;
        if overflow {
            let _ = child.kill().await;
        }
        let status = child
            .wait()
            .await
            .map_err(|_| "Git did not exit cleanly".to_string())?;
        if !status.success() && !overflow {
            return Err("Git command failed".to_string());
        }
        if overflow && !permit_truncation {
            return Err("Git output exceeded its 1 MiB safety limit".to_string());
        }
        bytes.truncate(MAX_GIT_OUTPUT_BYTES);
        Ok((bytes, overflow))
    };
    tokio::time::timeout(GIT_TIMEOUT, read)
        .await
        .map_err(|_| "Git command timed out".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "CA Test")
            .env("GIT_AUTHOR_EMAIL", "ca@example.invalid")
            .env("GIT_COMMITTER_NAME", "CA Test")
            .env("GIT_COMMITTER_EMAIL", "ca@example.invalid")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn strict_minimum_version_rejects_partial_and_compares_prereleases() {
        assert!(parse_minimum_version("1.2").is_err());
        assert!(parse_minimum_version("1.2.3 trailing").is_err());
        assert_eq!(
            parse_minimum_version("1.2.3-rc.1").unwrap().to_string(),
            "1.2.3-rc.1"
        );
        assert!(require_minimum_version(&Version::new(999, 0, 0)).is_err());
        assert!(require_minimum_version(&Version::new(0, 0, 0)).is_ok());
    }

    #[tokio::test]
    async fn git_context_is_bounded_marks_binary_and_never_reads_untracked_content() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        std::fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        std::fs::write(root.path().join("binary.bin"), [0_u8, 1, 2]).unwrap();
        git(root.path(), &["add", "tracked.txt", "binary.bin"]);
        git(root.path(), &["commit", "-q", "-m", "base"]);
        std::fs::write(root.path().join("tracked.txt"), "base\nchanged\n").unwrap();
        std::fs::write(root.path().join("binary.bin"), [0_u8, 9, 2]).unwrap();
        std::fs::write(root.path().join("untracked.txt"), "SECRET-UNTRACKED-BODY").unwrap();
        let workspace = WorkspaceContext::discover(root.path()).unwrap();
        let snapshot = capture_git_context(&workspace).await.unwrap();
        assert_eq!(snapshot.unstaged, 2);
        assert_eq!(snapshot.untracked, 1);
        assert!(snapshot.binary_files >= 1);
        assert!(snapshot.text.contains("tracked.txt"));
        assert!(snapshot.text.contains("untracked.txt"));
        assert!(!snapshot.text.contains("SECRET-UNTRACKED-BODY"));
        assert!(snapshot.text.len() <= MAX_GIT_CONTEXT_BYTES);
        let canonical_root = std::fs::canonicalize(root.path()).unwrap();
        assert!(snapshot
            .explicit_paths
            .iter()
            .all(|path| path.starts_with(&canonical_root)));
    }

    #[test]
    fn malformed_status_paths_fail_closed() {
        let root = Path::new("/tmp/repository");
        assert!(parse_status(root, b"?? ../escape\0").is_err());
        assert!(parse_status(root, b"R  old\0").is_err());
    }
}
