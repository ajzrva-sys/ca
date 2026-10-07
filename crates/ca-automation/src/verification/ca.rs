//! Shell-free verification recipes required by the `ca` integration.
//!
//! The parser accepts only the documented command shapes and normalizes flag
//! ordering. Repository-specific hygiene checks execute as Rust code through
//! the public `codex-team verify` command, never through a shell.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use walkdir::{DirEntry, WalkDir};

use crate::paths::Paths;
use crate::verification::recipe::{CommandKind, VerificationCommand};

const MAX_REPORTED_PATHS: usize = 64;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum BuiltinCheck {
    SnapshotHygiene,
    ProtectedTrees,
}

impl BuiltinCheck {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "snapshot-hygiene" => Some(Self::SnapshotHygiene),
            "protected-trees" => Some(Self::ProtectedTrees),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotHygiene => "snapshot-hygiene",
            Self::ProtectedTrees => "protected-trees",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub(crate) struct BuiltinCheckReceipt {
    pub(crate) schema: String,
    pub(crate) check: String,
    pub(crate) passed: bool,
    pub(crate) summary: String,
    pub(crate) paths: Vec<String>,
    pub(crate) truncated: bool,
}

/// Parse one of the exact `ca` completion recipes into a normalized command.
/// Any shell syntax, environment prefix, override, package selection, or
/// unrecognized flag fails closed.
pub(crate) fn parse_ca_verification_command(
    command: &str,
    source: &str,
) -> Option<VerificationCommand> {
    let words = strict_words(command)?;
    let normalized = match words.as_slice() {
        [program, args @ ..] if program == "cargo" => parse_cargo(args)?,
        [program, flag]
            if program == "target/release/ca"
                && matches!(flag.as_str(), "--help" | "--version") =>
        {
            (program.clone(), vec![flag.clone()], CommandKind::Smoke)
        }
        [program, command, flag] if program == "git" && command == "diff" && flag == "--check" => (
            "git".to_string(),
            vec!["diff".to_string(), "--check".to_string()],
            CommandKind::Git,
        ),
        [program, verify, name]
            if program == "codex-team"
                && verify == "verify"
                && BuiltinCheck::parse(name).is_some() =>
        {
            (
                "codex-team".to_string(),
                vec!["verify".to_string(), name.clone()],
                CommandKind::Lint,
            )
        }
        _ => return None,
    };

    let (program, args, kind) = normalized;
    let mut parsed = VerificationCommand::new(program, args, source);
    parsed.id = command_id(&parsed);
    parsed.kind = kind;
    parsed.expected_exit = Some(0);
    Some(parsed)
}

pub(crate) fn is_ca_verification_command(command: &VerificationCommand) -> bool {
    if command.cwd.is_some()
        || command.expected_exit.unwrap_or(0) != 0
        || command.timeout_seconds.unwrap_or(900) > 3_600
    {
        return false;
    }
    let display = std::iter::once(command.program.as_str())
        .chain(command.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    parse_ca_verification_command(&display, "validation")
        .is_some_and(|parsed| parsed.program == command.program && parsed.args == command.args)
}

pub(crate) fn run_builtin_check(paths: &Paths, check: BuiltinCheck) -> Result<BuiltinCheckReceipt> {
    match check {
        BuiltinCheck::SnapshotHygiene => snapshot_hygiene(paths),
        BuiltinCheck::ProtectedTrees => protected_trees(paths),
    }
}

pub(crate) fn require_builtin_check(
    paths: &Paths,
    check: BuiltinCheck,
) -> Result<BuiltinCheckReceipt> {
    let receipt = run_builtin_check(paths, check)?;
    if !receipt.passed {
        bail!("{}", receipt.summary)
    }
    Ok(receipt)
}

fn parse_cargo(args: &[String]) -> Option<(String, Vec<String>, CommandKind)> {
    let (subcommand, rest) = args.split_first()?;
    let normalized = match subcommand.as_str() {
        "test" => parse_cargo_test(rest)?,
        "fmt" if same_flags(rest, &["--all", "--check"]) => {
            vec![
                "fmt".to_string(),
                "--all".to_string(),
                "--check".to_string(),
            ]
        }
        "clippy"
            if same_flags_before_separator(
                rest,
                &["--all-targets", "--all-features"],
                &["-D", "warnings"],
            ) =>
        {
            [
                "clippy",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]
            .into_iter()
            .map(str::to_string)
            .collect()
        }
        "build" if same_flags(rest, &["--release", "--all-features"]) => {
            vec![
                "build".to_string(),
                "--release".to_string(),
                "--all-features".to_string(),
            ]
        }
        _ => return None,
    };
    Some((
        "cargo".to_string(),
        normalized,
        if subcommand == "test" {
            CommandKind::Test
        } else if subcommand == "build" {
            CommandKind::Build
        } else {
            CommandKind::Lint
        },
    ))
}

fn parse_cargo_test(rest: &[String]) -> Option<Vec<String>> {
    let separator = rest.iter().position(|token| token == "--");
    let (before, after) = match separator {
        Some(index) => (&rest[..index], Some(&rest[index + 1..])),
        None => (rest, None),
    };
    if after.is_some_and(|args| args != ["--nocapture"])
        || separator.is_some() && after.is_some_and(|args| args.is_empty())
    {
        return None;
    }

    let mut filter = None;
    let mut flags = Vec::new();
    for token in before {
        if token.starts_with('-') {
            if !matches!(
                token.as_str(),
                "--no-fail-fast" | "--all-targets" | "--all-features" | "--quiet"
            ) {
                return None;
            }
            flags.push(token.as_str());
        } else if filter.is_none() && safe_filter(token) {
            filter = Some(token.as_str());
        } else {
            return None;
        }
    }
    let flag_count = flags.len();
    flags.sort_unstable();
    flags.dedup();
    if flags.len() != flag_count {
        return None;
    }

    let no_fail_fast = flags.contains(&"--no-fail-fast");
    if !no_fail_fast {
        return None;
    }
    let all_targets = flags.contains(&"--all-targets");
    let all_features = flags.contains(&"--all-features");
    let quiet = flags.contains(&"--quiet");

    let mut normalized = vec!["test".to_string()];
    if let Some(filter) = filter {
        if all_targets || all_features || quiet {
            return None;
        }
        normalized.push(filter.to_string());
        normalized.push("--no-fail-fast".to_string());
        if after.is_some() {
            normalized.extend(["--".to_string(), "--nocapture".to_string()]);
        }
        return Some(normalized);
    }
    if after.is_some() || all_targets != all_features || quiet && !all_targets {
        return None;
    }
    if all_targets {
        normalized.extend(["--all-targets".to_string(), "--all-features".to_string()]);
    }
    normalized.push("--no-fail-fast".to_string());
    if quiet {
        normalized.push("--quiet".to_string());
    }
    Some(normalized)
}

fn same_flags(got: &[String], expected: &[&str]) -> bool {
    if got.len() != expected.len() {
        return false;
    }
    let mut got = got.iter().map(String::as_str).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    got.sort_unstable();
    expected.sort_unstable();
    got == expected
}

fn same_flags_before_separator(got: &[String], before: &[&str], after: &[&str]) -> bool {
    let Some(index) = got.iter().position(|token| token == "--") else {
        return false;
    };
    same_flags(&got[..index], before) && got[index + 1..] == *after
}

fn safe_filter(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 256
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':')
        })
}

fn strict_words(command: &str) -> Option<Vec<String>> {
    if command.is_empty()
        || command.len() > 4 * 1024
        || command.chars().any(|ch| {
            ch.is_control()
                || matches!(
                    ch,
                    '|' | '&'
                        | ';'
                        | '<'
                        | '>'
                        | '$'
                        | '`'
                        | '\\'
                        | '\''
                        | '"'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                )
        })
    {
        return None;
    }
    let words = command
        .split_ascii_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    (!words.is_empty()).then_some(words)
}

fn command_id(command: &VerificationCommand) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-ca-verification-v1\0");
    hasher.update(command.program.as_bytes());
    for arg in &command.args {
        hasher.update(b"\0");
        hasher.update(arg.as_bytes());
    }
    let digest = hasher.finalize();
    let mut short = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(&mut short, "{byte:02x}");
    }
    format!("ca-recipe:{short}")
}

fn snapshot_hygiene(paths: &Paths) -> Result<BuiltinCheckReceipt> {
    let mut matches = Vec::new();
    let mut total = 0usize;
    for entry in WalkDir::new(&paths.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
    {
        let entry = entry.with_context(|| "walk repository for pending snapshots")?;
        if entry.file_type().is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".snap.new"))
        {
            total += 1;
            if matches.len() < MAX_REPORTED_PATHS {
                matches.push(relative_display(&paths.root, entry.path()));
            }
        }
    }
    matches.sort();
    Ok(check_receipt(
        BuiltinCheck::SnapshotHygiene,
        total,
        matches,
        "no pending .snap.new files",
        "pending .snap.new files",
    ))
}

fn protected_trees(paths: &Paths) -> Result<BuiltinCheckReceipt> {
    let mut unverifiable = Vec::new();
    let mut present_roots = Vec::new();
    let mut ignored_roots = Vec::new();
    for root in ["oracles", "reference"] {
        let path = paths.root.join(root);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).context("inspect protected tree root"),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            unverifiable.push(format!("{root}/ (protected root is not a real directory)"));
            continue;
        }
        present_roots.push(root.to_string());
        let probe = format!("{root}/.codex-team-protected-probe");
        let ignored = crate::git_metadata::command_no_filters(&paths.root)?
            .args(["check-ignore", "--no-index", "-q", "--", &probe])
            .current_dir(&paths.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("inspect protected tree ignore policy")?;
        match ignored.code() {
            Some(0) => ignored_roots.push(root.to_string()),
            Some(1) => {}
            _ => bail!("git check-ignore failed for a protected tree"),
        }
    }
    unverifiable.extend(crate::verification::protected_tree::verify(
        paths,
        &present_roots,
        &ignored_roots,
    )?);
    let output = crate::git_metadata::command_no_filters(&paths.root)?
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            "oracles",
            "reference",
            ".codex-team-protected-trees.json",
        ])
        .current_dir(&paths.root)
        .output()
        .context("inspect protected tree Git state")?;
    if !output.status.success() {
        bail!(
            "git status failed for protected trees: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
    let all = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    let total = all.len() + unverifiable.len();
    let mut matches = unverifiable;
    matches.extend(
        all.into_iter()
            .take(MAX_REPORTED_PATHS.saturating_sub(matches.len()))
            .map(bounded_lossy),
    );
    matches.sort();
    Ok(check_receipt(
        BuiltinCheck::ProtectedTrees,
        total,
        matches,
        "oracles/ and reference/ are unchanged",
        "changes under protected trees",
    ))
}

fn check_receipt(
    check: BuiltinCheck,
    total: usize,
    paths: Vec<String>,
    passed_summary: &str,
    failed_summary: &str,
) -> BuiltinCheckReceipt {
    let passed = total == 0;
    BuiltinCheckReceipt {
        schema: "codex-team.verify.v1".to_string(),
        check: check.as_str().to_string(),
        passed,
        summary: if passed {
            passed_summary.to_string()
        } else {
            format!("found {total} {failed_summary}")
        },
        truncated: total > paths.len(),
        paths,
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    !matches!(
        entry.file_name().to_str(),
        Some(".git" | ".orchestrator" | "target" | "oracles" | "reference")
    )
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn bounded_lossy(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace(['\r', '\n', '\0'], "?");
    let mut out = text.chars().take(512).collect::<String>();
    if text.chars().count() > 512 {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn accepts_and_normalizes_every_ca_recipe() {
        let cases = [
            ("cargo test --no-fail-fast", "cargo test --no-fail-fast"),
            (
                "cargo test --all-features --no-fail-fast --all-targets",
                "cargo test --all-targets --all-features --no-fail-fast",
            ),
            (
                "cargo test --quiet --all-targets --no-fail-fast --all-features",
                "cargo test --all-targets --all-features --no-fail-fast --quiet",
            ),
            (
                "cargo test role_run --no-fail-fast",
                "cargo test role_run --no-fail-fast",
            ),
            (
                "cargo test role_run --no-fail-fast -- --nocapture",
                "cargo test role_run --no-fail-fast -- --nocapture",
            ),
            ("cargo fmt --check --all", "cargo fmt --all --check"),
            (
                "cargo clippy --all-features --all-targets -- -D warnings",
                "cargo clippy --all-targets --all-features -- -D warnings",
            ),
            (
                "cargo build --all-features --release",
                "cargo build --release --all-features",
            ),
            ("target/release/ca --help", "target/release/ca --help"),
            ("target/release/ca --version", "target/release/ca --version"),
            ("git diff --check", "git diff --check"),
            (
                "codex-team verify snapshot-hygiene",
                "codex-team verify snapshot-hygiene",
            ),
            (
                "codex-team verify protected-trees",
                "codex-team verify protected-trees",
            ),
        ];
        for (input, expected) in cases {
            let parsed = parse_ca_verification_command(input, "test")
                .unwrap_or_else(|| panic!("recipe should parse: {input}"));
            assert_eq!(parsed.display(), expected);
            assert!(is_ca_verification_command(&parsed));
        }
    }

    #[test]
    fn rejects_authority_widening_and_shell_syntax() {
        let rejected = [
            "RUSTFLAGS=-Awarnings cargo test --no-fail-fast",
            "cargo test --manifest-path ../other/Cargo.toml --no-fail-fast",
            "cargo test --target-dir /tmp/out --no-fail-fast",
            "cargo test -p secret --no-fail-fast",
            "cargo clippy --all-targets --all-features -- -A warnings",
            "cargo build --release --all-features && echo bad",
            "cargo test --no-fail-fast | tee log",
            "sh -c cargo test",
            "curl https://example.invalid",
            "target/release/other --help",
            "codex-team verify unknown",
        ];
        for input in rejected {
            assert!(
                parse_ca_verification_command(input, "test").is_none(),
                "recipe should reject: {input}"
            );
        }
    }

    #[test]
    fn builtin_checks_report_pending_and_protected_changes() -> Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-ca-check-{nonce}"));
        fs::create_dir_all(root.join("oracles"))?;
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .stdin(Stdio::null())
            .status()?;
        assert!(status.success());
        let paths = Paths::new(&root)?;

        assert!(run_builtin_check(&paths, BuiltinCheck::SnapshotHygiene)?.passed);
        fs::create_dir_all(root.join("src/snapshots"))?;
        fs::write(root.join("src/snapshots/view.snap.new"), "pending")?;
        let snapshots = run_builtin_check(&paths, BuiltinCheck::SnapshotHygiene)?;
        assert!(!snapshots.passed);
        assert_eq!(snapshots.paths, vec!["src/snapshots/view.snap.new"]);

        fs::write(root.join("oracles/changed.txt"), "no")?;
        let protected = run_builtin_check(&paths, BuiltinCheck::ProtectedTrees)?;
        assert!(!protected.passed);
        assert!(protected
            .paths
            .iter()
            .any(|path| path.contains("oracles/changed.txt")));

        fs::remove_file(root.join("oracles/changed.txt"))?;
        fs::write(root.join(".gitignore"), "/oracles/\n")?;
        let ignored = run_builtin_check(&paths, BuiltinCheck::ProtectedTrees)?;
        assert!(!ignored.passed);
        assert!(ignored.paths.iter().any(|path| {
            path.contains("oracles/") && path.contains("committed content baseline is missing")
        }));
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
