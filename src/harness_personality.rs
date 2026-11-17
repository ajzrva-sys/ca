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

pub const PERSONALITY_HEADING: &str = "## Personality";
pub const PRE_PROMPT_HEADING: &str = "## Pre-prompt";

/// Voice rules adapted from the user's Hermes identity line. The Hermes and
/// Nous names are not part of ca.
pub fn seeded_personality() -> &'static str {
    "Match the length of the reply to the weight of the ask. A one-line question gets a one-line answer. Finished work gets a short report of what changed, what is verified, and what is left, never a replay of the process. No filler, no restating the request, and no narrating tool calls the user can see. Plain claims over adjectives. When unsure, say so plainly. Agree because it is right, not because the user said it. Give depth when the user asks for detail, teaches, or the stakes demand it, not by default."
}

/// The selected Hermes personality was `technical`. The other built-ins are not used.
pub fn seeded_pre_prompt() -> &'static str {
    "Be a technical expert. Provide detailed, accurate technical information."
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteeringSections {
    pub personality: String,
    pub pre_prompt: String,
}

pub fn steering_sections(text: &str) -> SteeringSections {
    SteeringSections {
        personality: section_body(text, PERSONALITY_HEADING),
        pre_prompt: section_body(text, PRE_PROMPT_HEADING),
    }
}

/// Editor view of a document. A missing heading shows the seed. An existing
/// heading is returned as stored, including when the user cleared its body.
pub fn display_sections(text: Option<&str>) -> SteeringSections {
    let Some(text) = text else {
        return SteeringSections {
            personality: seeded_personality().to_string(),
            pre_prompt: seeded_pre_prompt().to_string(),
        };
    };
    let mut sections = steering_sections(text);
    if find_heading(text, PERSONALITY_HEADING).is_none() {
        sections.personality = seeded_personality().to_string();
    }
    if find_heading(text, PRE_PROMPT_HEADING).is_none() {
        sections.pre_prompt = seeded_pre_prompt().to_string();
    }
    sections
}

fn section_body(text: &str, heading: &str) -> String {
    let Some(start) = find_heading(text, heading) else {
        return String::new();
    };
    let rest = &text[start + heading.len()..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].trim().to_string()
}

fn find_heading(text: &str, heading: &str) -> Option<usize> {
    if text.starts_with(heading) && text[heading.len()..].starts_with(['\n', '\r']) {
        return Some(0);
    }
    text.find(&format!("\n{heading}\n"))
        .or_else(|| text.find(&format!("\n{heading}\r\n")))
        .map(|index| index + 1)
}

pub fn upsert_sections(text: &str, personality: &str, pre_prompt: &str) -> String {
    let text = upsert_section(text, PERSONALITY_HEADING, personality);
    upsert_section(&text, PRE_PROMPT_HEADING, pre_prompt)
}

fn upsert_section(text: &str, heading: &str, body: &str) -> String {
    let body = body.trim();
    if let Some(start) = find_heading(text, heading) {
        let after_heading = start + heading.len();
        let rest = text[after_heading..]
            .strip_prefix('\n')
            .unwrap_or(&text[after_heading..]);
        let body_start = text.len() - rest.len();
        let end = rest
            .find("\n## ")
            .map(|index| body_start + index)
            .unwrap_or(text.len());
        let mut out = String::new();
        out.push_str(&text[..body_start]);
        if !body.is_empty() {
            out.push_str(body);
            out.push('\n');
        }
        out.push_str(&text[end..]);
        return out;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(heading);
    out.push('\n');
    if !body.is_empty() {
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// Fill a missing Personality or Pre-prompt heading. An existing heading is left
/// alone, including when its body is empty.
pub fn ensure_seeded(paths: &RuntimePaths, profile: &ActiveProfile) -> Result<bool> {
    if matches!(profile, ActiveProfile::Bare) {
        return Ok(false);
    }
    ensure_seeded_at(&path(paths, profile))
}

pub fn ensure_seeded_at(target: &std::path::Path) -> Result<bool> {
    let current = read_at(target)?.unwrap_or_else(|| template().to_string());
    let sections = steering_sections(&current);
    let missing_personality = find_heading(&current, PERSONALITY_HEADING).is_none();
    let missing_pre_prompt = find_heading(&current, PRE_PROMPT_HEADING).is_none();
    if !missing_personality && !missing_pre_prompt {
        return Ok(false);
    }
    let next = upsert_sections(
        &current,
        if missing_personality {
            seeded_personality()
        } else {
            &sections.personality
        },
        if missing_pre_prompt {
            seeded_pre_prompt()
        } else {
            &sections.pre_prompt
        },
    );
    write_document(target, &next)?;
    Ok(true)
}

pub fn save_sections_at(
    target: &std::path::Path,
    personality: &str,
    pre_prompt: &str,
) -> Result<()> {
    if personality.len() + pre_prompt.len() > MAX_BYTES as usize {
        bail!("personality text exceeds 32 KiB");
    }
    let current = read_at(target)?.unwrap_or_else(|| template().to_string());
    let next = upsert_sections(&current, personality, pre_prompt);
    if next.len() as u64 > MAX_BYTES {
        bail!("personality.md exceeds 32 KiB");
    }
    write_document(target, &next)
}

fn write_document(target: &std::path::Path, text: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        let mut directory = fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(parent)?;
    }
    let temporary = target.with_extension("md.tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    {
        let mut file = options.open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&temporary, target)?;
    Ok(())
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
    read_at(&path(paths, profile))
}

pub fn read_at(target: &std::path::Path) -> Result<Option<String>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(target) {
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
    // Headings and comments are not content. CA's seed paragraphs are content
    // for the editor, and they are not a user answer.
    let answered = content.lines().any(substantive_line);
    Ok(answered.then(|| content.trim().to_string()))
}

fn substantive_line(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && !line.starts_with('#')
}

fn generated_steering_line(line: &str) -> bool {
    let line = line.trim();
    !substantive_line(line) || line == seeded_personality() || line == seeded_pre_prompt()
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
    let _ = ensure_seeded(paths, profile);
    let mut text = String::new();
    if let Some(personality) = load(paths, profile)? {
        // Seed paragraphs alone are not user-authored steering.
        if personality
            .lines()
            .any(|line| !generated_steering_line(line))
        {
            text.push_str("\n\n## User-authored CA personality and behavior\nApply these preferences to how you work and communicate. The current user's explicit instructions take precedence. These preferences do not add tools or permission grants.\n");
            text.push_str(&personality);
        }
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
        let unanswered = prompt(&paths, &profile, root.path(), "fake", "fake").unwrap();
        assert!(
            !unanswered.contains("User-authored CA personality and behavior"),
            "{unanswered}"
        );
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

    #[test]
    fn hermes_voice_seeds_empty_sections_and_leaves_answered_text() {
        let root = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(root.path());
        let profile = ActiveProfile::Default;
        initialize(&paths, &profile).unwrap();
        assert!(ensure_seeded(&paths, &profile).unwrap());
        assert!(!ensure_seeded(&paths, &profile).unwrap());
        let loaded = load(&paths, &profile).unwrap().unwrap();
        assert!(loaded.contains("one-line question"));
        assert!(loaded.contains("technical expert"));
        assert!(!loaded.contains("Hermes"));
        assert!(!loaded.contains("Nous"));
        let voice = "# Voice\nKeep updates brief.\n";
        fs::write(path(&paths, &profile), voice).unwrap();
        assert!(ensure_seeded(&paths, &profile).unwrap());
        let stored = fs::read_to_string(path(&paths, &profile)).unwrap();
        assert!(stored.contains("Keep updates brief."));
        let cleared = "# Voice\nKeep updates brief.\n\n## Personality\n\n## Pre-prompt\n\n";
        fs::write(path(&paths, &profile), cleared).unwrap();
        assert!(!ensure_seeded(&paths, &profile).unwrap());
        let stored = fs::read_to_string(path(&paths, &profile)).unwrap();
        assert!(stored.contains("Keep updates brief."));
        assert!(!stored.contains("one-line question"));
        let shown = display_sections(Some(&stored));
        assert!(shown.personality.is_empty());
        assert!(shown.pre_prompt.is_empty());
        save_sections_at(
            &path(&paths, &profile),
            "Shorter replies.",
            "Stay technical.",
        )
        .unwrap();
        let stored = fs::read_to_string(path(&paths, &profile)).unwrap();
        assert!(stored.contains("Keep updates brief."));
        assert!(stored.contains("Shorter replies."));
        assert!(stored.contains("Stay technical."));
        assert!(!stored.contains("one-line question"));
    }
}
