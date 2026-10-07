//! Git coordinator commands must not execute repository or ambient helpers.
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Construct a fixed Git process with no inherited credentials, Git environment
/// overrides, global/system configuration, hooks, fsmonitor or credential helper.
/// Callers still own typed arguments, scope, timeout and bounded output.
pub fn command(root: &Path) -> Command {
    let mut command = Command::new("/usr/bin/git");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .args([
            "--no-pager",
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "credential.helper=",
            "-c",
            "diff.external=",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "tag.gpgSign=false",
            "-c",
            "merge.verifySignatures=false",
        ])
        .current_dir(root);
    command
}

/// Fixed metadata queries also disable repository diff and textconv drivers.
pub fn query(root: &Path, args: &[&str]) -> Result<Command> {
    let mut command = command_no_filters(root)?;
    if args.first() == Some(&"diff") {
        command
            .args(["diff", "--no-ext-diff", "--no-textconv"])
            .args(&args[1..]);
    } else {
        command.args(args);
    }
    Ok(command)
}

/// Local attributes can name clean/process filters even with hooks disabled.
/// Inventory bounded key names and suppress every configured driver before
/// trusted staging, worktree creation or capture. Values never leave Git.
pub fn command_no_filters(root: &Path) -> Result<Command> {
    const MAX_KEYS_BYTES: u64 = 64 * 1024;
    const MAX_ERROR_BYTES: u64 = 16 * 1024;
    let mut child = command(root)
        .args([
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|process|smudge|required)$",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("inventory configured Git filter key names")?;
    let stdout = child.stdout.take().context("Git filter inventory stdout")?;
    let stderr = child.stderr.take().context("Git filter inventory stderr")?;
    let read = |reader: Box<dyn Read + Send>, limit: u64| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            reader.take(limit + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        })
    };
    let stdout = read(Box::new(stdout), MAX_KEYS_BYTES);
    let stderr = read(Box::new(stderr), MAX_ERROR_BYTES);
    let deadline = Instant::now() + Duration::from_secs(5);
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Ok(status), false),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                let _ = child.kill();
                break (child.wait(), true);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break (Err(error), false);
            }
        }
    };
    let output = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("Git filter inventory reader stopped"))??;
    let error_output = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("Git filter inventory error reader stopped"))??;
    let status = status?;
    if timed_out
        || output.len() as u64 > MAX_KEYS_BYTES
        || error_output.len() as u64 > MAX_ERROR_BYTES
    {
        bail!("Git filter inventory exceeded its execution or output bound");
    }
    if !(status.success() || (status.code() == Some(1) && output.is_empty())) {
        bail!("configured Git filter inventory is unavailable");
    }
    let mut drivers = BTreeSet::new();
    for key in output
        .split(|byte| *byte == 0)
        .filter(|key| !key.is_empty())
    {
        let key = std::str::from_utf8(key).context("Git filter key is not UTF-8")?;
        let Some((driver, _field)) = key
            .strip_prefix("filter.")
            .and_then(|key| key.rsplit_once('.'))
        else {
            bail!("Git filter inventory returned an invalid key");
        };
        if driver.is_empty() || driver.len() > 512 || driver.chars().any(char::is_control) {
            bail!("configured Git filter driver name is outside its bound");
        }
        drivers.insert(driver.to_string());
        if drivers.len() > 128 {
            bail!("configured Git filter count exceeds its bound");
        }
    }
    let mut command = command(root);
    for driver in drivers {
        for field in ["clean", "process", "smudge"] {
            command.args(["-c", &format!("filter.{driver}.{field}=")]);
        }
        command.args(["-c", &format!("filter.{driver}.required=false")]);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{bail, Result};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn git(root: &Path, args: &[&str]) -> Result<()> {
        let result = query(root, args)?.output()?;
        if !result.status.success() {
            bail!("fixture Git command failed: {}", result.status);
        }
        Ok(())
    }

    #[test]
    fn revision_query_and_builtin_metadata_never_execute_repository_helpers() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let repo = temporary.path().join("repo");
        fs::create_dir(&repo)?;
        git(&repo, &["init", "--quiet"])?;
        fs::write(repo.join("tracked.txt"), "before\n")?;
        fs::write(repo.join(".gitattributes"), "tracked.txt diff=canary\n")?;
        git(&repo, &["add", "tracked.txt", ".gitattributes"])?;
        git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ],
        )?;
        let marker = temporary.path().join("coordinator-executed-helper");
        let helper = temporary.path().join("helper");
        fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
        )?;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))?;
        let helper = helper.to_str().unwrap();
        for name in ["core.fsmonitor", "diff.canary.textconv", "diff.external"] {
            git(&repo, &["config", name, helper])?;
        }
        git(&repo, &["config", "filter.canary.clean", helper])?;
        fs::write(
            repo.join(".gitattributes"),
            "tracked.txt diff=canary filter=canary\n",
        )?;
        fs::write(repo.join("tracked.txt"), "after\n")?;
        let paths = crate::paths::Paths::new(&repo)?;
        let before = crate::revision::repository_revision(&paths)?;
        crate::mechanistic::execute_query_action(
            &repo,
            0,
            &ca_automation_protocol::MechanisticActionV1::GitStatusSummary { max_entries: 20 },
            8192,
        )?;
        crate::mechanistic::execute_query_action(
            &repo,
            1,
            &ca_automation_protocol::MechanisticActionV1::GitDiffSummary { max_entries: 20 },
            8192,
        )?;
        crate::verification::ca::run_builtin_check(
            &paths,
            crate::verification::ca::BuiltinCheck::ProtectedTrees,
        )?;
        assert!(!marker.exists());
        assert_eq!(fs::read_to_string(repo.join("tracked.txt"))?, "after\n");
        assert_eq!(crate::revision::repository_revision(&paths)?, before);
        Ok(())
    }

    #[test]
    fn trusted_staging_suppresses_clean_and_process_filter_helpers() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let repo = temporary.path().join("repo");
        fs::create_dir(&repo)?;
        git(&repo, &["init", "--quiet"])?;
        let marker = temporary.path().join("filter-executed");
        let helper = temporary.path().join("filter");
        fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )?;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))?;
        let helper = helper.to_str().unwrap();
        for field in ["clean", "process", "smudge"] {
            git(
                &repo,
                &["config", &format!("filter.fixture.{field}"), helper],
            )?;
        }
        git(&repo, &["config", "filter.fixture.required", "true"])?;
        fs::write(repo.join(".gitattributes"), "filtered.txt filter=fixture\n")?;
        fs::write(repo.join("filtered.txt"), "candidate bytes\n")?;
        let output = command_no_filters(&repo)?.args(["add", "--all"]).output()?;
        assert!(output.status.success(), "staging failed: {}", output.status);
        assert!(!marker.exists());
        let staged = query(&repo, &["show", ":filtered.txt"])?.output()?;
        assert!(staged.status.success());
        assert_eq!(staged.stdout, b"candidate bytes\n");
        Ok(())
    }
}
