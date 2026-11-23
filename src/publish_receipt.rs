//! Host-owned receipt for a literal `git push`. The model does not compute it.
use crate::parity_backend::shell_words;
use crate::undo::{FileMutationReceipt, FileState};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{LazyLock, Mutex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PublishReceipt {
    pub toplevel: PathBuf,
    pub head: String,
    pub parent: String,
    pub remote_sha: String,
    pub matches_remote: bool,
    pub paths_added: u32,
    pub paths_modified: u32,
    pub paths_deleted: u32,
    pub lines_inserted: u32,
    pub lines_deleted: u32,
    pub lockfile_dependency_change: bool,
    pub pushed_command: String,
    pub paths: Vec<String>,
    remote_name: String,
    remote_ref: String,
    expected_sha: String,
}

#[derive(Clone, Debug)]
struct PendingCommand {
    command: String,
    cwd: PathBuf,
}

static PENDING: LazyLock<Mutex<HashMap<crate::jobs::JobOwner, Vec<PendingCommand>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn note_command(owner: crate::jobs::JobOwner, command: &str, cwd: &str) {
    if !matches!(
        scan_command(command, Path::new(cwd)),
        Scan::Hidden | Scan::Pushes(_)
    ) {
        return;
    }
    PENDING
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .entry(owner)
        .or_default()
        .push(PendingCommand {
            command: command.to_string(),
            cwd: PathBuf::from(cwd),
        });
}

pub(crate) fn take(owner: crate::jobs::JobOwner) -> Vec<(String, PathBuf)> {
    PENDING
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .remove(&owner)
        .unwrap_or_default()
        .into_iter()
        .map(|pending| (pending.command, pending.cwd))
        .collect()
}

#[derive(Debug)]
enum Scan {
    None,
    Hidden,
    Pushes(Vec<PushRequest>),
}

#[derive(Debug)]
struct PushRequest {
    cwd: PathBuf,
    command: String,
    positionals: Vec<String>,
}

pub(crate) enum ProbeBatch {
    None,
    Hidden,
    Receipts(Vec<PublishReceipt>),
}

pub(crate) fn probe_commands(commands: &[(String, PathBuf)]) -> Result<ProbeBatch, String> {
    let mut receipts = Vec::new();
    for (command, cwd) in commands {
        match scan_command(command, cwd) {
            Scan::None => {}
            Scan::Hidden => return Ok(ProbeBatch::Hidden),
            Scan::Pushes(pushes) => {
                for push in pushes {
                    receipts.push(probe_push(push)?);
                }
            }
        }
    }
    if receipts.is_empty() {
        Ok(ProbeBatch::None)
    } else {
        Ok(ProbeBatch::Receipts(receipts))
    }
}

pub(crate) fn refresh_remote(receipt: &mut PublishReceipt) {
    let sha =
        ls_remote(&receipt.toplevel, &receipt.remote_name, &receipt.remote_ref).unwrap_or_default();
    receipt.remote_sha = sha.clone();
    receipt.matches_remote = !sha.is_empty() && sha == receipt.expected_sha;
}

pub(crate) fn format_blocks(receipts: &[PublishReceipt], covers: bool, approved: bool) -> String {
    receipts
        .iter()
        .map(|receipt| {
            format!(
                "PUBLISH_RECEIPT\nhead={}\nremote={}\nmatch={}\npaths_added={}\npaths_modified={}\npaths_deleted={}\nlines_inserted={}\nlines_deleted={}\nlockfile_dependency_change={}\nreview_covers_repository={}\nreview_approved={}\npushed_command={}",
                receipt.head,
                receipt.remote_sha,
                receipt.matches_remote,
                receipt.paths_added,
                receipt.paths_modified,
                receipt.paths_deleted,
                receipt.lines_inserted,
                receipt.lines_deleted,
                receipt.lockfile_dependency_change,
                covers,
                approved,
                one_line(&receipt.pushed_command),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) const PUSH_THIS_COMMIT: &str = "Push this commit";
pub(crate) const LEAVE_IT_LOCAL: &str = "Leave it local";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HostPublishCard {
    pub turn: uuid::Uuid,
    pub text: String,
    pub choices: [&'static str; 2],
}

/// The host card replaces any model question while a receipt belongs to this turn.
pub(crate) fn host_publish_card(
    receipts: &[PublishReceipt],
    turn: uuid::Uuid,
    model_question: Option<&str>,
) -> Option<HostPublishCard> {
    if receipts.is_empty() {
        return None;
    }
    let _ = model_question;
    Some(HostPublishCard {
        turn,
        text: format_blocks(receipts, true, false),
        choices: [PUSH_THIS_COMMIT, LEAVE_IT_LOCAL],
    })
}

pub(crate) fn resolve_host_publish_choice(
    receipts: &[PublishReceipt],
    card_turn: uuid::Uuid,
    answer_turn: uuid::Uuid,
    choice: &str,
) -> Result<(), String> {
    if receipts.is_empty() {
        return Err("no publish receipt".into());
    }
    if answer_turn != card_turn {
        return Err("only the turn that produced the receipt can answer".into());
    }
    if choice != PUSH_THIS_COMMIT && choice != LEAVE_IT_LOCAL {
        return Err("the model cannot add a choice".into());
    }
    if choice == PUSH_THIS_COMMIT && receipts.iter().any(|receipt| !receipt.matches_remote) {
        return Err("publish remote does not match HEAD".into());
    }
    Ok(())
}

pub(crate) fn bind_approval(
    report: &mut crate::automatic_review::ReviewReport,
    receipts: &[PublishReceipt],
    covers: bool,
) {
    if receipts.is_empty() {
        return;
    }
    let remote_ok = receipts.iter().all(|receipt| receipt.matches_remote);
    if covers && remote_ok {
        return;
    }
    report.approved = false;
    let message = if !remote_ok {
        "publish remote does not match HEAD"
    } else {
        "review candidate does not include the pushed commit"
    };
    if !report
        .findings
        .iter()
        .any(|finding| finding.path == "publish receipt")
    {
        report
            .findings
            .push(crate::automatic_review::ReviewFinding {
                path: "publish receipt".into(),
                message: message.into(),
            });
    }
}

pub(crate) fn commit_files(receipt: &PublishReceipt) -> Result<Vec<FileMutationReceipt>, String> {
    let mut files = Vec::new();
    for path in &receipt.paths {
        let before = show_file(&receipt.toplevel, &receipt.parent, path)?;
        let after = show_file(&receipt.toplevel, &receipt.head, path)?;
        if before == after {
            continue;
        }
        files.push(FileMutationReceipt::new(
            receipt.toplevel.join(path),
            "git push",
            before,
            after,
        ));
    }
    Ok(files)
}

pub(crate) fn covered(receipt: &PublishReceipt, files: &[PathBuf]) -> bool {
    receipt.paths.iter().all(|path| {
        let absolute = receipt.toplevel.join(path);
        files.iter().any(|candidate| candidate == &absolute)
    })
}

fn one_line(command: &str) -> String {
    command.replace(['\n', '\r'], " ")
}

fn scan_command(command: &str, cwd: &Path) -> Scan {
    let mut base = cwd.to_path_buf();
    let mut trusted = true;
    let mut pushes = Vec::new();
    let mut hidden = false;
    for segment in crate::parity_backend::split_command_segments(command) {
        if let Some(dir) = cd_argument(&segment) {
            if trusted && path_is_literal(&dir) {
                let path = PathBuf::from(&dir);
                base = if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                };
            } else {
                trusted = false;
            }
            continue;
        }
        if segment_hides_push(&segment) {
            hidden = true;
        }
        if let Some(positionals) = literal_push(&segment) {
            if trusted {
                pushes.push(PushRequest {
                    cwd: base.clone(),
                    command: command.to_string(),
                    positionals,
                });
            } else {
                hidden = true;
            }
        }
    }
    if hidden {
        Scan::Hidden
    } else if pushes.is_empty() {
        Scan::None
    } else {
        Scan::Pushes(pushes)
    }
}

fn path_is_literal(word: &str) -> bool {
    !word.is_empty() && !word.contains(['$', '`', '*', '?', '~'])
}

fn cd_argument(segment: &str) -> Option<String> {
    let words = shell_words(segment);
    let program = words.first()?.rsplit(['/', '\\']).next()?;
    if program != "cd" {
        return None;
    }
    let rest: Vec<&String> = words
        .iter()
        .skip(1)
        .filter(|word| word.as_str() != "--")
        .collect();
    match rest.as_slice() {
        [dir] => Some((*dir).clone()),
        _ => Some(String::new()),
    }
}

fn segment_hides_push(segment: &str) -> bool {
    let words = shell_words(segment);
    let program = words
        .first()
        .map(|word| word.rsplit(['/', '\\']).next().unwrap_or(word.as_str()))
        .unwrap_or("");
    if matches!(program, "bash" | "sh" | "zsh" | "dash")
        && words.iter().any(|word| word == "-c" || word == "-lc")
    {
        return words
            .iter()
            .any(|word| word.contains("git") && word.contains("push"));
    }
    if segment.contains("$(") || segment.contains('`') || segment.contains("${") {
        return segment.contains("git") && segment.contains("push");
    }
    false
}

fn literal_push(segment: &str) -> Option<Vec<String>> {
    if segment_hides_push(segment) {
        return None;
    }
    let words = shell_words(segment);
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        if word.contains('=') && !word.starts_with('-') {
            let key = word.split('=').next().unwrap_or("");
            if !key.is_empty()
                && key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                index += 1;
                continue;
            }
        }
        break;
    }
    let program = words.get(index)?.rsplit(['/', '\\']).next()?;
    if program != "git" {
        return None;
    }
    index += 1;
    while index < words.len() && words[index] != "push" {
        if words[index] == "-C" {
            index += 2;
            continue;
        }
        return None;
    }
    if words.get(index)? != "push" {
        return None;
    }
    index += 1;
    let mut positionals = Vec::new();
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            positionals.extend(words[index + 1..].iter().cloned());
            break;
        }
        if word.starts_with('-') {
            if matches!(
                word.as_str(),
                "-o" | "--push-option" | "--receive-pack" | "--exec" | "--repo"
            ) {
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        positionals.push(word.clone());
        index += 1;
    }
    Some(positionals)
}

fn probe_push(push: PushRequest) -> Result<PublishReceipt, String> {
    let toplevel = git_text(&push.cwd, &["rev-parse", "--show-toplevel"])?;
    let toplevel =
        std::fs::canonicalize(toplevel.trim()).unwrap_or_else(|_| PathBuf::from(toplevel.trim()));
    let head = git_text(&toplevel, &["rev-parse", "HEAD"])?;
    let head = head.trim().to_string();
    let parent = git_text(&toplevel, &["rev-parse", "--verify", "HEAD^"])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(empty_tree);
    let (remote_name, remote_ref, expected_sha) =
        remote_endpoint(&toplevel, &head, &push.positionals)?;
    let remote_sha = ls_remote(&toplevel, &remote_name, &remote_ref).unwrap_or_default();
    let name_status = git_bytes(
        &toplevel,
        &[
            "diff",
            "--no-renames",
            "--name-status",
            "-z",
            &parent,
            &head,
        ],
    )?;
    let numstat = git_bytes(
        &toplevel,
        &["diff", "--no-renames", "--numstat", "-z", &parent, &head],
    )?;
    let (paths, paths_added, paths_modified, paths_deleted) = parse_name_status(&name_status)?;
    let (lines_inserted, lines_deleted) = parse_numstat(&numstat)?;
    let package = root_package_name(&toplevel, &head).unwrap_or_default();
    let lock_diff = git_text(
        &toplevel,
        &["diff", "-U1", &parent, &head, "--", "Cargo.lock"],
    )
    .unwrap_or_default();
    Ok(PublishReceipt {
        toplevel,
        head,
        parent,
        remote_sha: remote_sha.clone(),
        matches_remote: !remote_sha.is_empty() && remote_sha == expected_sha,
        paths_added,
        paths_modified,
        paths_deleted,
        lines_inserted,
        lines_deleted,
        lockfile_dependency_change: lockfile_dependency_change(&lock_diff, &package),
        pushed_command: push.command,
        paths,
        remote_name,
        remote_ref,
        expected_sha,
    })
}

fn empty_tree() -> String {
    "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into()
}

fn remote_endpoint(
    toplevel: &Path,
    head: &str,
    positionals: &[String],
) -> Result<(String, String, String), String> {
    let branch = git_text(toplevel, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| text != "HEAD");
    let (remote, spec) = match positionals {
        [] => ("origin".to_string(), branch.clone().unwrap_or_default()),
        [one]
            if one.contains(':')
                || one == "HEAD"
                || one.starts_with("refs/")
                || branch.as_ref() == Some(one) =>
        {
            ("origin".to_string(), one.clone())
        }
        [one] => (one.clone(), branch.clone().unwrap_or_default()),
        [remote, spec, ..] => (remote.clone(), spec.clone()),
    };
    let local = spec.trim_start_matches('+');
    let (source, dest) = local.split_once(':').unwrap_or((local, local));
    let remote_ref = if dest == "HEAD" {
        branch
            .as_deref()
            .map(|name| format!("refs/heads/{name}"))
            .unwrap_or_default()
    } else if dest.starts_with("refs/") {
        dest.to_string()
    } else if dest.is_empty() {
        String::new()
    } else {
        format!("refs/heads/{dest}")
    };
    let expected = if source.is_empty() || source == "HEAD" {
        head.to_string()
    } else {
        git_text(toplevel, &["rev-parse", source])
            .map(|text| text.trim().to_string())
            .unwrap_or_default()
    };
    if remote_ref.is_empty() || expected.is_empty() {
        return Err("publish remote ref could not be read".into());
    }
    Ok((remote, remote_ref, expected))
}

fn ls_remote(toplevel: &Path, remote: &str, remote_ref: &str) -> Result<String, String> {
    let text = git_text(toplevel, &["ls-remote", remote, remote_ref])?;
    Ok(text
        .lines()
        .find_map(|line| {
            let (sha, name) = line.split_once(['\t', ' '])?;
            (name.trim() == remote_ref).then(|| sha.trim().to_string())
        })
        .unwrap_or_default())
}

fn parse_name_status(bytes: &[u8]) -> Result<(Vec<String>, u32, u32, u32), String> {
    let mut paths = Vec::new();
    let mut added = 0;
    let mut modified = 0;
    let mut deleted = 0;
    let mut parts = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty());
    while let Some(status) = parts.next() {
        let path = parts
            .next()
            .ok_or("publish name-status is missing a path")?;
        let path = String::from_utf8(path.to_vec()).map_err(|_| "publish path is not utf-8")?;
        let status = String::from_utf8_lossy(status);
        let kind = status.chars().next().unwrap_or('M');
        match kind {
            'A' => added += 1,
            'D' => deleted += 1,
            _ => modified += 1,
        }
        paths.push(path);
    }
    Ok((paths, added, modified, deleted))
}

fn parse_numstat(bytes: &[u8]) -> Result<(u32, u32), String> {
    let mut inserted = 0u32;
    let mut deleted = 0u32;
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let record = String::from_utf8_lossy(record);
        let mut fields = record.split('\t');
        let added = fields.next().unwrap_or("0");
        let removed = fields.next().unwrap_or("0");
        if added == "-" || removed == "-" {
            return Err("publish path is not reviewable text".into());
        }
        inserted = inserted.saturating_add(added.parse().unwrap_or(0));
        deleted = deleted.saturating_add(removed.parse().unwrap_or(0));
    }
    Ok((inserted, deleted))
}

fn root_package_name(toplevel: &Path, head: &str) -> Result<String, String> {
    let text = git_text(toplevel, &["show", &format!("{head}:Cargo.toml")])?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(name) = line.strip_prefix("name") {
                let name = name.trim().trim_start_matches('=').trim().trim_matches('"');
                if !name.is_empty() {
                    return Ok(name.to_string());
                }
            }
        }
    }
    Err("root package name is missing".into())
}

pub(crate) fn lockfile_dependency_change(diff: &str, package: &str) -> bool {
    if diff.trim().is_empty() {
        return false;
    }
    let mut current = String::new();
    for line in diff.lines() {
        if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("@@") {
            continue;
        }
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        let (marker, body) = if let Some(body) = line.strip_prefix('+') {
            ('+', body)
        } else if let Some(body) = line.strip_prefix('-') {
            ('-', body)
        } else if let Some(body) = line.strip_prefix(' ') {
            (' ', body)
        } else {
            continue;
        };
        if let Some(name) = package_name(body) {
            if marker != '-' {
                current = name;
            }
        }
        if marker == ' ' {
            continue;
        }
        let version = body.trim_start().strip_prefix("version");
        let version_line = version.is_some_and(|rest| rest.trim_start().starts_with('='));
        if !version_line || current != package || package.is_empty() {
            return true;
        }
    }
    false
}

fn package_name(body: &str) -> Option<String> {
    let body = body.trim();
    let rest = body.strip_prefix("name")?;
    let rest = rest.trim().strip_prefix('=')?.trim().trim_matches('"');
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn show_file(toplevel: &Path, rev: &str, path: &str) -> Result<FileState, String> {
    let spec = format!("{rev}:{path}");
    let output = git(toplevel, &["show", &spec])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist") || stderr.contains("exists on disk") {
            return Ok(FileState::Missing);
        }
        return Err(format!("publish path could not be read: {path}"));
    }
    if output.stdout.len() > crate::undo::MAX_UNDO_FILE_BYTES {
        return Err("publish diff exceeds its review bound".into());
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| format!("publish path is not reviewable text: {path}"))?;
    Ok(FileState::Utf8(text))
}

fn git(cwd: &Path, args: &[&str]) -> Result<Output, String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| error.to_string())
}

fn git_text(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = git(cwd, args)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git(cwd, args)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_at(cwd: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn a_version_only_lock_change_is_not_a_dependency_change() {
        let diff = "\
diff --git a/Cargo.lock b/Cargo.lock
@@ -1,3 +1,3 @@
 [[package]]
 name = \"ca\"
-version = \"0.1.174\"
+version = \"0.1.190\"
";
        assert!(!lockfile_dependency_change(diff, "ca"));
        let dependency = "\
 name = \"libc\"
-version = \"0.2.0\"
+version = \"0.2.1\"
";
        assert!(lockfile_dependency_change(dependency, "ca"));
        assert!(!lockfile_dependency_change("", "ca"));
    }

    #[test]
    fn a_hidden_push_is_not_a_receipt_and_a_literal_push_matches_the_remote() {
        assert!(matches!(
            scan_command("bash -c 'git push origin main'", Path::new("/tmp")),
            Scan::Hidden
        ));
        assert!(matches!(
            scan_command("echo git push", Path::new("/tmp")),
            Scan::None
        ));
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("remote.git");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        git_at(
            dir.path(),
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        git_at(&work, &["init", "-b", "main"]);
        git_at(&work, &["config", "user.email", "t@example.com"]);
        git_at(&work, &["config", "user.name", "t"]);
        std::fs::write(
            work.join("Cargo.toml"),
            "[package]\nname = \"ca\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            work.join("Cargo.lock"),
            "[[package]]\nname = \"ca\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(work.join("note.txt"), "one\n").unwrap();
        git_at(&work, &["add", "."]);
        git_at(&work, &["commit", "-m", "init"]);
        std::fs::write(
            work.join("Cargo.lock"),
            "[[package]]\nname = \"ca\"\nversion = \"0.1.1\"\n",
        )
        .unwrap();
        std::fs::write(work.join("note.txt"), "two\n").unwrap();
        git_at(&work, &["add", "."]);
        git_at(&work, &["commit", "-m", "bump"]);
        git_at(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git_at(&work, &["push", "origin", "main"]);
        let batch = probe_commands(&[("git push origin main".into(), work.clone())]).unwrap();
        let ProbeBatch::Receipts(receipts) = batch else {
            panic!("literal push");
        };
        let receipt = &receipts[0];
        assert!(receipt.matches_remote);
        assert!(!receipt.lockfile_dependency_change);
        assert_eq!(receipt.paths_modified, 2);
        assert_eq!(receipt.paths_added, 0);
        assert_eq!(receipt.paths_deleted, 0);
        assert!(receipt.lines_inserted > 0);
        assert!(receipt.lines_deleted > 0);
        let files = commit_files(receipt).unwrap();
        assert_eq!(files.len(), 2);
        let mut report = crate::automatic_review::ReviewReport {
            approved: true,
            findings: Vec::new(),
            summary: "session logs only".into(),
        };
        bind_approval(&mut report, receipts.as_slice(), false);
        assert!(!report.approved);
        assert_eq!(report.findings[0].path, "publish receipt");
        let block = format_blocks(receipts.as_slice(), true, true);
        assert!(block.contains("match=true"));
        assert!(block.contains("lockfile_dependency_change=false"));
        assert!(block.contains("review_covers_repository=true"));
        std::fs::write(
            work.join("Cargo.lock"),
            "[[package]]\nname = \"ca\"\nversion = \"0.1.1\"\n\n[[package]]\nname = \"libc\"\nversion = \"0.2.1\"\n",
        )
        .unwrap();
        git_at(&work, &["add", "Cargo.lock"]);
        git_at(&work, &["commit", "-m", "dep"]);
        git_at(&work, &["push", "origin", "main"]);
        let again = probe_commands(&[("git push origin main".into(), work)]).unwrap();
        let ProbeBatch::Receipts(deps) = again else {
            panic!("dependency push");
        };
        assert!(deps[0].lockfile_dependency_change);
    }

    fn sample_receipt(matches_remote: bool) -> PublishReceipt {
        PublishReceipt {
            toplevel: PathBuf::from("/work"),
            head: "abc".into(),
            parent: "def".into(),
            remote_sha: if matches_remote { "abc" } else { "zzz" }.into(),
            matches_remote,
            paths_added: 0,
            paths_modified: 1,
            paths_deleted: 0,
            lines_inserted: 1,
            lines_deleted: 0,
            lockfile_dependency_change: false,
            pushed_command: "git push origin main".into(),
            paths: vec!["note.txt".into()],
            remote_name: "origin".into(),
            remote_ref: "refs/heads/main".into(),
            expected_sha: "abc".into(),
        }
    }

    #[test]
    fn host_publish_card_keeps_the_receipt_text_and_refuses_a_remote_mismatch() {
        let turn = uuid::Uuid::new_v4();
        let later = uuid::Uuid::new_v4();
        let matched = vec![sample_receipt(true)];
        let card = host_publish_card(
            &matched,
            turn,
            Some("Push this commit? with a third choice"),
        )
        .unwrap();
        assert_eq!(card.choices, [PUSH_THIS_COMMIT, LEAVE_IT_LOCAL]);
        assert_eq!(card.text, format_blocks(&matched, true, false));
        assert!(!card.text.contains("third choice"));
        assert!(card.text.contains("head=abc"));
        assert!(card.text.contains("pushed_command=git push origin main"));
        resolve_host_publish_choice(&matched, turn, turn, PUSH_THIS_COMMIT).unwrap();
        let mismatched = vec![sample_receipt(false)];
        let card = host_publish_card(&mismatched, turn, None).unwrap();
        assert!(card.text.contains("remote=zzz"));
        assert!(
            resolve_host_publish_choice(&mismatched, turn, turn, PUSH_THIS_COMMIT)
                .unwrap_err()
                .contains("remote")
        );
        assert!(
            resolve_host_publish_choice(&mismatched, turn, later, LEAVE_IT_LOCAL)
                .unwrap_err()
                .contains("turn")
        );
        resolve_host_publish_choice(&mismatched, turn, turn, LEAVE_IT_LOCAL).unwrap();
    }
}
