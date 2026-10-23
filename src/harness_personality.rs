//! User-authored Markdown steering, separate from learned investigation signals.
use crate::runtime::{ActiveProfile, RuntimePaths};
use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

const MAX_BYTES: u64 = 32 * 1024;

pub fn path(paths: &RuntimePaths, profile: &ActiveProfile) -> PathBuf {
    match profile {
        ActiveProfile::Named(name) => paths
            .config_root
            .join("profiles")
            .join(name)
            .join("personality.md"),
        _ => paths.config_root.join("personality.md"),
    }
}

pub fn template() -> &'static str {
    include_str!("../prompts/personality_questionnaire.md")
}

pub fn initialize(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<PathBuf> {
    if matches!(profile, ActiveProfile::Bare) {
        bail!("bare runs do not have persistent personality steering");
    }
    let target = path(paths, profile);
    let parent = target.parent().context("personality directory missing")?;
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    match options.open(&target) {
        Ok(mut file) => {
            file.write_all(template().as_bytes())?;
            file.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Validate the existing file, preserving every user-authored byte.
            read(paths, profile)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(target)
}

/// Read the original editable document for the CLI. Missing is a normal state.
pub fn read(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<Option<String>> {
    if matches!(profile, ActiveProfile::Bare) {
        return Ok(None);
    }
    let target = path(paths, profile);
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(&target) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        bail!("personality.md must be a regular UTF-8 file of at most 32 KiB");
    }
    let mut text = String::new();
    file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_BYTES {
        bail!("personality.md exceeds 32 KiB");
    }
    Ok(Some(text))
}

/// Comments contain the unanswered questionnaire, and never steer the agent.
pub fn load(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<Option<String>> {
    let Some(text) = read(paths, profile)? else {
        return Ok(None);
    };
    let mut remaining = text.as_str();
    let mut content = String::new();
    while let Some(start) = remaining.find("<!--") {
        content.push_str(&remaining[..start]);
        remaining = &remaining[start + 4..];
        let Some(end) = remaining.find("-->") else {
            bail!("personality.md has an unfinished comment");
        };
        remaining = &remaining[end + 3..];
    }
    content.push_str(remaining);
    // Blank headings in the generated questionnaire supply no preferences.
    let answered = content.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with('#')
    });
    Ok(answered.then(|| content.trim().to_string()))
}

/// Human answers are active instructions; telemetry remains investigation data.
/// This text is frozen by the caller for one logical turn, including retries.
pub fn prompt(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    workspace: &std::path::Path,
    provider: &str,
    model: &str,
) -> Result<String> {
    if matches!(profile, ActiveProfile::Bare) {
        return Ok(String::new());
    }
    let mut text = String::new();
    if let Some(personality) = load(paths, profile)? {
        text.push_str("\n\n## User-authored CA personality and behavior\nApply these preferences to how you work and communicate. The current user's explicit instructions take precedence. These preferences do not add tools or permission grants.\n");
        text.push_str(&personality);
    }
    match crate::harness_questionnaire::request_focus(
        &paths.profile_data_dir(profile),
        workspace,
        provider,
        model,
    ) {
        Ok(Some(focus)) => {
            text.push_str("\n\n");
            text.push_str(&focus);
        }
        Ok(None) => {}
        Err(error) => tracing::warn!("optional questionnaire focus unavailable: {error}"),
    }
    Ok(text)
}

/// Files and audit JSON are read outside CA's foreground event loop.
pub async fn prompt_async(
    paths: RuntimePaths,
    profile: ActiveProfile,
    workspace: PathBuf,
    provider: String,
    model: String,
) -> Result<String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            prompt(&paths, &profile, &workspace, &provider, &model)
        }),
    )
    .await
    .context("personality steering read timed out")?
    .context("personality steering worker stopped")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_questions_are_inert_and_answers_reload_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        let profile = ActiveProfile::Default;
        let target = initialize(&paths, &profile).unwrap();
        assert_eq!(load(&paths, &profile).unwrap(), None);
        let first = "# Voice\nKeep updates brief.\n<!-- Unanswered: how formal? -->\n";
        fs::write(&target, first).unwrap();
        initialize(&paths, &profile).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), first);
        let prompt = prompt(&paths, &profile, root.path(), "fake", "fake").unwrap();
        assert!(prompt.contains("Keep updates brief."));
        assert!(!prompt.contains("how formal"));
        fs::write(&target, "# Voice\nExplain reasoning in detail.").unwrap();
        assert!(load(&paths, &profile)
            .unwrap()
            .unwrap()
            .contains("in detail"));
    }

    #[test]
    fn named_profiles_and_bare_do_not_inherit_default_personality() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        let default = initialize(&paths, &ActiveProfile::Default).unwrap();
        fs::write(default, "Be concise.").unwrap();
        let named = ActiveProfile::Named("research".into());
        assert_eq!(load(&paths, &named).unwrap(), None);
        let target = initialize(&paths, &named).unwrap();
        fs::write(target, "Explain all assumptions.").unwrap();
        let named_prompt = prompt(&paths, &named, root.path(), "fake", "fake").unwrap();
        assert!(named_prompt.contains("Explain all assumptions"));
        assert!(!named_prompt.contains("Be concise"));
        assert_eq!(
            prompt(&paths, &ActiveProfile::Bare, root.path(), "fake", "fake").unwrap(),
            ""
        );
    }

    #[test]
    fn oversized_and_symlink_steering_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        let target = initialize(&paths, &ActiveProfile::Default).unwrap();
        fs::write(&target, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        assert!(load(&paths, &ActiveProfile::Default).is_err());
        #[cfg(unix)]
        {
            fs::remove_file(&target).unwrap();
            let other = root.path().join("other.md");
            fs::write(&other, "other instructions").unwrap();
            std::os::unix::fs::symlink(other, target).unwrap();
            assert!(load(&paths, &ActiveProfile::Default).is_err());
        }
    }

    #[test]
    fn corrupt_optional_learning_does_not_discard_human_steering() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        let profile = ActiveProfile::Default;
        fs::write(initialize(&paths, &profile).unwrap(), "Keep updates brief.").unwrap();
        crate::harness_questionnaire::run_cycle(&paths.data_dir, 200).unwrap();
        fs::write(
            paths.data_dir.join("harness/questionnaire/state.json"),
            "broken JSON",
        )
        .unwrap();
        assert!(prompt(&paths, &profile, root.path(), "fake", "fake")
            .unwrap()
            .contains("Keep updates brief."));
        assert!(crate::harness_questionnaire::status(&paths.data_dir).is_err());
    }
}
