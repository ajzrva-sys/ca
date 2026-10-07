//! Inert reusable prompt commands discovered from user, profile, and workspace roots.
//!
//! A prompt command only expands text into the ordinary composer submission path. It
//! never executes a shell or grants tools, permissions, models, or network access.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_COMMAND_BYTES: u64 = 64 * 1024;
const MAX_COMMANDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PromptCommandScope {
    Workspace,
    User,
    ProfileAutoDistill,
    Compatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptCommand {
    pub name: String,
    pub description: String,
    pub agent: Option<String>,
    pub body: String,
    pub path: PathBuf,
    pub scope: PromptCommandScope,
    pub auto_distilled: bool,
    pub run_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PromptCommandRegistry {
    commands: Vec<PromptCommand>,
}

impl PromptCommandRegistry {
    pub(crate) fn names(&self) -> BTreeMap<String, PathBuf> {
        self.commands
            .iter()
            .map(|command| (command.name.clone(), command.path.clone()))
            .collect()
    }

    pub(crate) fn discover(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        workspace: &Path,
    ) -> Self {
        let mut roots = vec![
            (
                PromptCommandScope::Workspace,
                workspace.join(".ca/commands"),
            ),
            (PromptCommandScope::Workspace, workspace.join(".ca/command")),
            (PromptCommandScope::User, paths.home.join(".ca/commands")),
            (
                PromptCommandScope::ProfileAutoDistill,
                paths.auto_distilled_commands_dir(profile),
            ),
        ];
        roots.extend(
            crate::foreign_knowledge::foreign_prompt_command_roots(&paths.home, workspace)
                .into_iter()
                .map(|path| (PromptCommandScope::Compatibility, path)),
        );
        Self::discover_roots(&roots)
    }

    pub(crate) fn discover_for_workspace(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
        workspace: &crate::workspace::WorkspaceContext,
    ) -> Self {
        let mut roots = Vec::new();
        for root in workspace.roots() {
            roots.push((
                PromptCommandScope::Workspace,
                root.path.join(".ca/commands"),
            ));
            roots.push((PromptCommandScope::Workspace, root.path.join(".ca/command")));
        }
        roots.push((PromptCommandScope::User, paths.home.join(".ca/commands")));
        roots.push((
            PromptCommandScope::ProfileAutoDistill,
            paths.auto_distilled_commands_dir(profile),
        ));
        if let Some(primary) = workspace.primary() {
            roots.extend(
                crate::foreign_knowledge::foreign_prompt_command_roots(&paths.home, &primary.path)
                    .into_iter()
                    .map(|path| (PromptCommandScope::Compatibility, path)),
            );
        }
        Self::discover_roots(&roots)
    }

    pub(crate) fn discover_user_only(
        paths: &crate::runtime::RuntimePaths,
        profile: &crate::runtime::ActiveProfile,
    ) -> Self {
        Self::discover_roots(&[
            (PromptCommandScope::User, paths.home.join(".ca/commands")),
            (
                PromptCommandScope::ProfileAutoDistill,
                paths.auto_distilled_commands_dir(profile),
            ),
        ])
    }

    fn discover_roots(roots: &[(PromptCommandScope, PathBuf)]) -> Self {
        let mut selected = BTreeMap::<String, PromptCommand>::new();
        for (scope, root) in roots {
            let mut files = Vec::new();
            collect_markdown(root, root, &mut files, 0);
            files.sort();
            for path in files {
                if selected.len() >= MAX_COMMANDS {
                    break;
                }
                let Ok(command) = parse_command_file(&path, *scope) else {
                    continue;
                };
                if crate::tui::commands::find_command(&command.name).is_some() {
                    continue;
                }
                selected.entry(command.name.clone()).or_insert(command);
            }
        }
        Self {
            commands: selected.into_values().collect(),
        }
    }

    /// Add unambiguous portable skills to the ordinary slash palette. The
    /// expansion only inserts an exact `$qualified-skill` selector into the
    /// normal prompt path; it does not execute a script or grant authority.
    pub(crate) fn include_skills(mut self, skills: &crate::skills::SkillRegistry) -> Self {
        let mut counts = BTreeMap::<String, usize>::new();
        for instance in skills
            .instances()
            .filter(|instance| instance.enabled && instance.availability.compatible())
        {
            *counts.entry(instance.metadata.name.clone()).or_default() += 1;
        }
        let mut selected = self
            .commands
            .drain(..)
            .map(|command| (command.name.clone(), command))
            .collect::<BTreeMap<_, _>>();
        for instance in skills
            .instances()
            .filter(|instance| instance.enabled && instance.availability.compatible())
        {
            if selected.len() >= MAX_COMMANDS
                || counts.get(&instance.metadata.name).copied() != Some(1)
                || crate::tui::commands::find_command(&instance.metadata.name).is_some()
                || selected.contains_key(&instance.metadata.name)
            {
                continue;
            }
            let scope = match instance.source_scope {
                crate::skills::SkillSourceScope::Workspace
                | crate::skills::SkillSourceScope::Legacy => PromptCommandScope::Workspace,
                crate::skills::SkillSourceScope::AutoDistill => {
                    PromptCommandScope::ProfileAutoDistill
                }
                _ => PromptCommandScope::User,
            };
            selected.insert(
                instance.metadata.name.clone(),
                PromptCommand {
                    name: instance.metadata.name.clone(),
                    description: instance.metadata.description.clone(),
                    agent: None,
                    body: format!("${} $ARGUMENTS", instance.qualified_name),
                    path: PathBuf::from(format!("<skill:{}>", instance.instance_id)),
                    scope,
                    auto_distilled: instance.source_scope
                        == crate::skills::SkillSourceScope::AutoDistill,
                    run_id: None,
                },
            );
        }
        self.commands = selected.into_values().collect();
        self
    }

    pub(crate) fn include_bundles(
        mut self,
        bundles: &crate::skill_bundles::SkillBundleRegistry,
    ) -> Self {
        let mut selected = self
            .commands
            .drain(..)
            .map(|command| (command.name.clone(), command))
            .collect::<BTreeMap<_, _>>();
        for bundle in bundles.bundles() {
            if selected.len() >= MAX_COMMANDS
                || selected.contains_key(&bundle.name)
                || crate::tui::commands::find_command(&bundle.name).is_some()
            {
                continue;
            }
            let mut body = bundle
                .members
                .iter()
                .map(|member| format!("${}", member.instance_id))
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(instruction) = &bundle.instruction {
                body.push_str("\n<bundle-guidance untrusted-data=\"true\">\n");
                body.push_str(instruction);
                body.push_str("\n</bundle-guidance>");
            }
            body.push_str("\n$ARGUMENTS");
            selected.insert(
                bundle.name.clone(),
                PromptCommand {
                    name: bundle.name.clone(),
                    description: bundle.description.clone(),
                    agent: None,
                    body,
                    path: bundle.path.clone(),
                    scope: match bundle.scope {
                        crate::skill_bundles::BundleScope::Workspace => {
                            PromptCommandScope::Workspace
                        }
                        crate::skill_bundles::BundleScope::Profile => PromptCommandScope::User,
                        crate::skill_bundles::BundleScope::Foreign => {
                            PromptCommandScope::Compatibility
                        }
                    },
                    auto_distilled: false,
                    run_id: None,
                },
            );
        }
        self.commands = selected.into_values().collect();
        self
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn commands(&self) -> &[PromptCommand] {
        &self.commands
    }

    pub(crate) fn find(&self, name: &str) -> Option<&PromptCommand> {
        let name = name.trim().to_ascii_lowercase();
        self.commands.iter().find(|command| command.name == name)
    }

    pub(crate) fn matching(&self, query: &str, busy: bool) -> Vec<&PromptCommand> {
        if busy {
            return Vec::new();
        }
        let query = query.trim().to_ascii_lowercase();
        let mut commands = self
            .commands
            .iter()
            .filter(|command| {
                query.is_empty()
                    || command.name.contains(&query)
                    || command.description.to_ascii_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        commands
    }

    pub(crate) fn expand_line(&self, line: &str) -> Result<Option<String>, String> {
        let line = line.trim().trim_start_matches('/').trim();
        let (name, arguments) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let Some(command) = self.find(name) else {
            return Ok(None);
        };
        Ok(Some(command.expand(arguments.trim())?))
    }
}

impl PromptCommand {
    pub(crate) fn expand(&self, arguments: &str) -> Result<String, String> {
        if arguments.len() > MAX_COMMAND_BYTES as usize {
            return Err(format!("/{} arguments are too large", self.name));
        }
        let positional = arguments.split_whitespace().take(9).collect::<Vec<_>>();
        let mut expanded = self.body.replace("$ARGUMENTS", arguments);
        for index in (1..=9).rev() {
            expanded = expanded.replace(
                &format!("${index}"),
                positional.get(index - 1).copied().unwrap_or_default(),
            );
        }
        if let Some(agent) = &self.agent {
            expanded = format!(
                "Use the `{agent}` specialist for this reusable prompt when delegation is useful.\n\n{expanded}"
            );
        }
        if expanded.trim().is_empty() || expanded.len() > MAX_COMMAND_BYTES as usize * 2 {
            return Err(format!("/{} expanded to an invalid prompt", self.name));
        }
        Ok(expanded)
    }
}

pub(crate) fn render_native_command(description: &str, body: &str) -> Result<String, String> {
    render_native_command_with_agent(description, None, body)
}

pub(crate) fn render_native_command_with_agent(
    description: &str,
    agent: Option<&str>,
    body: &str,
) -> Result<String, String> {
    if description.is_empty()
        || description.len() > 240
        || description.chars().any(char::is_control)
    {
        return Err("command description is unsafe".into());
    }
    let body = body.trim();
    if body.is_empty()
        || body.len() > MAX_COMMAND_BYTES as usize
        || crate::learning::sanitized_learning_text(body).as_deref() != Some(body)
    {
        return Err("command prompt is unsafe".into());
    }
    if let Some(agent) = agent {
        validate_name(agent)?;
    }
    let agent = agent.map_or_else(String::new, |agent| format!("agent: {agent}\n"));
    Ok(format!(
        "---\ndescription: {}\n{}---\n\n{}\n",
        description, agent, body
    ))
}

fn collect_markdown(root: &Path, directory: &Path, output: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 || output.len() >= MAX_COMMANDS || !directory.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.file_type().is_dir() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            collect_markdown(root, &path, output, depth + 1);
        } else if metadata.file_type().is_file()
            && metadata.len() <= MAX_COMMAND_BYTES
            && path.extension().is_some_and(|extension| extension == "md")
            && path.starts_with(root)
        {
            output.push(path);
        }
    }
}

fn parse_command_file(path: &Path, scope: PromptCommandScope) -> Result<PromptCommand, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "command could not be inspected")?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_COMMAND_BYTES
    {
        return Err("command is not a bounded regular file".into());
    }
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "command name is invalid".to_string())?
        .to_ascii_lowercase();
    validate_name(&name)?;
    let content = fs::read_to_string(path).map_err(|_| "command could not be read")?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let mut description = None;
    let mut agent = None;
    let mut origin = None;
    let mut run_id = None;
    for line in frontmatter.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err("command frontmatter is invalid".into());
        };
        let value = value.trim();
        match key.trim() {
            "description" if !value.is_empty() => description = Some(value.to_string()),
            "agent" if !value.is_empty() => {
                validate_name(value)?;
                agent = Some(value.to_string());
            }
            "ca_origin" if !value.is_empty() => origin = Some(value.to_string()),
            "ca_run_id" if !value.is_empty() => {
                run_id = Some(
                    uuid::Uuid::parse_str(value)
                        .map_err(|_| "command run provenance is invalid".to_string())?,
                );
            }
            _ => return Err("command frontmatter contains an unsupported field".into()),
        }
    }
    let description = description.ok_or_else(|| "command description is missing".to_string())?;
    if crate::learning::sanitized_learning_text(&description).as_deref()
        != Some(description.as_str())
        || description.len() > 240
    {
        return Err("command description is unsafe".into());
    }
    let body = body.trim().to_string();
    if body.is_empty()
        || body.len() > MAX_COMMAND_BYTES as usize
        || crate::learning::sanitized_learning_text(&body).as_deref() != Some(body.as_str())
    {
        return Err("command prompt is unsafe".into());
    }
    let auto_distilled = origin.as_deref() == Some("auto_distill");
    if auto_distilled != run_id.is_some() || (origin.is_some() && !auto_distilled) {
        return Err("command provenance is invalid".into());
    }
    Ok(PromptCommand {
        name,
        description,
        agent,
        body,
        path: path.to_path_buf(),
        scope,
        auto_distilled,
        run_id,
    })
}

fn split_frontmatter(content: &str) -> Result<(&str, &str), String> {
    let content = content.trim();
    let rest = content
        .strip_prefix("---")
        .ok_or_else(|| "command frontmatter is missing".to_string())?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| "command frontmatter is unterminated".to_string())?;
    Ok((&rest[..end], &rest[end + 4..]))
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(
            "command name must use lowercase letters, digits, hyphens, or underscores".into(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PromptAssetPublishOutcome {
    pub created: Vec<PathBuf>,
    pub conflicts: Vec<String>,
    pub previous: BTreeMap<PathBuf, PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct PromptAssetWrite {
    pub target: PathBuf,
    pub content: String,
    pub previous: Option<PathBuf>,
    pub previous_digest: Option<String>,
}

impl PromptAssetWrite {
    pub(crate) fn digest(&self) -> String {
        content_digest(self.content.as_bytes())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PromptAssetPublishPlan {
    pub writes: Vec<PromptAssetWrite>,
    pub conflicts: Vec<String>,
}

pub(crate) fn plan_auto_distilled_commands(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    workspace: &Path,
    run_id: uuid::Uuid,
    commands: &[crate::learning::DistillCommandPackage],
) -> Result<PromptAssetPublishPlan, String> {
    let mut plan = PromptAssetPublishPlan::default();
    for command in commands {
        validate_name(&command.name)?;
        let root = match command.scope {
            crate::learning::ReusableAssetScope::Workspace => workspace.join(".ca/commands"),
            crate::learning::ReusableAssetScope::Profile => {
                paths.auto_distilled_commands_dir(profile)
            }
        };
        let target = root.join(format!("{}.md", command.name));
        if !target.starts_with(&root) {
            return Err("generated command escaped its publication root".into());
        }
        let content = render_generated_command(command, run_id);
        if target.exists() {
            let existing = parse_command_file(
                &target,
                match command.scope {
                    crate::learning::ReusableAssetScope::Workspace => PromptCommandScope::Workspace,
                    crate::learning::ReusableAssetScope::Profile => {
                        PromptCommandScope::ProfileAutoDistill
                    }
                },
            );
            let expected = command.replaces_digest.as_deref();
            let replaceable = existing.as_ref().is_ok_and(|existing| {
                existing.auto_distilled
                    && expected.is_some_and(|digest| {
                        digest == content_digest(&fs::read(&target).unwrap_or_default())
                    })
            });
            if !replaceable {
                plan.conflicts.push(format!(
                    "Command `{}` already exists and is human-owned or lacks exact generated provenance; extend existing instead",
                    command.name
                ));
                continue;
            }
            let archive = root
                .join(".generations")
                .join(run_id.to_string())
                .join(format!("{}.md", command.name));
            let previous = fs::read(&target).map_err(|error| error.to_string())?;
            plan.writes.push(PromptAssetWrite {
                target,
                content,
                previous: Some(archive),
                previous_digest: Some(content_digest(&previous)),
            });
        } else {
            plan.writes.push(PromptAssetWrite {
                target,
                content,
                previous: None,
                previous_digest: None,
            });
        }
    }
    Ok(plan)
}

pub(crate) fn plan_auto_distilled_agents(
    paths: &crate::runtime::RuntimePaths,
    profile: &crate::runtime::ActiveProfile,
    workspace: &Path,
    run_id: uuid::Uuid,
    agents: &[crate::learning::DistillAgentPackage],
) -> Result<PromptAssetPublishPlan, String> {
    let mut plan = PromptAssetPublishPlan::default();
    for agent in agents {
        validate_name(&agent.name)?;
        const READ_ONLY_TOOLS: &[&str] = &[
            "read_file",
            "search_files",
            "glob",
            "list_dir",
            "read_todos",
        ];
        if agent.tools.is_empty()
            || agent.tools.len() > READ_ONLY_TOOLS.len()
            || agent
                .tools
                .iter()
                .any(|tool| !READ_ONLY_TOOLS.contains(&tool.as_str()))
        {
            return Err("generated subagent requested a non-read-only tool".into());
        }
        let root = match agent.scope {
            crate::learning::ReusableAssetScope::Workspace => workspace.join(".ca/agents"),
            crate::learning::ReusableAssetScope::Profile => {
                paths.auto_distilled_agents_dir(profile)
            }
        };
        let target = root.join(format!("{}.md", agent.name));
        let content = format!(
            "---\nname: {}\ndescription: {}\ntools: {}\nca_origin: auto_distill\nca_run_id: {}\n---\n\n{}\n",
            agent.name,
            agent.description.replace('\n', " "),
            agent.tools.join(", "),
            run_id,
            agent.instructions.trim()
        );
        if target.exists() {
            let existing = fs::read(&target).map_err(|error| error.to_string())?;
            let replaceable = String::from_utf8_lossy(&existing)
                .contains("ca_origin: auto_distill")
                && agent
                    .replaces_digest
                    .as_deref()
                    .is_some_and(|digest| digest == content_digest(&existing));
            if !replaceable {
                plan.conflicts.push(format!(
                    "Subagent `{}` already exists and is human-owned or lacks exact generated provenance; extend existing instead",
                    agent.name
                ));
                continue;
            }
            let archive = root
                .join(".generations")
                .join(run_id.to_string())
                .join(format!("{}.md", agent.name));
            plan.writes.push(PromptAssetWrite {
                target,
                content,
                previous: Some(archive),
                previous_digest: Some(content_digest(&existing)),
            });
        } else {
            plan.writes.push(PromptAssetWrite {
                target,
                content,
                previous: None,
                previous_digest: None,
            });
        }
    }
    Ok(plan)
}

pub(crate) fn publish_prompt_asset_plan(
    plan: &PromptAssetPublishPlan,
) -> Result<PromptAssetPublishOutcome, String> {
    let mut outcome = PromptAssetPublishOutcome {
        conflicts: plan.conflicts.clone(),
        ..PromptAssetPublishOutcome::default()
    };
    for write in &plan.writes {
        if let Some(archive) = &write.previous {
            let previous = fs::read(&write.target).map_err(|error| error.to_string())?;
            if write.previous_digest.as_deref() != Some(content_digest(&previous).as_str()) {
                return Err("generated prompt asset changed after transaction planning".into());
            }
            write_new_file(archive, &String::from_utf8_lossy(&previous))?;
            replace_atomic(&write.target, &write.content)?;
            outcome
                .previous
                .insert(write.target.clone(), archive.clone());
        } else {
            write_new_file(&write.target, &write.content)?;
        }
        outcome.created.push(write.target.clone());
    }
    Ok(outcome)
}

fn render_generated_command(
    command: &crate::learning::DistillCommandPackage,
    run_id: uuid::Uuid,
) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("description: {}", command.description.replace('\n', " ")),
    ];
    if let Some(agent) = &command.agent {
        lines.push(format!("agent: {agent}"));
    }
    lines.extend([
        "ca_origin: auto_distill".into(),
        format!("ca_run_id: {run_id}"),
        "---".into(),
        String::new(),
        command.body.trim().to_string(),
        String::new(),
    ]);
    lines.join("\n")
}

fn write_new_file(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "generated command has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}

fn replace_atomic(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "generated command has no parent directory".to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    write_new_file(&temporary, content)?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        error.to_string()
    })
}

fn content_digest(content: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_command(root: &Path, name: &str, body: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join(format!("{name}.md")),
            format!("---\ndescription: reusable {name}\n---\n{body}\n"),
        )
        .unwrap();
    }

    #[test]
    fn discovers_commands_expands_arguments_literally_and_rejects_builtin_shadowing() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let commands = workspace.join(".ca/commands");
        write_command(
            &commands,
            "check-bsd",
            "Inspect $1 with all input: $ARGUMENTS",
        );
        write_command(&commands, "help", "shadow built-in");
        let registry =
            PromptCommandRegistry::discover_roots(&[(PromptCommandScope::Workspace, commands)]);
        assert_eq!(registry.commands().len(), 1);
        assert_eq!(
            registry
                .expand_line("/check-bsd host;rm -rf nope")
                .unwrap()
                .unwrap(),
            "Inspect host;rm with all input: host;rm -rf nope"
        );
        assert!(registry.find("help").is_none());
    }

    #[test]
    fn workspace_commands_win_and_symlinks_are_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let profile = temp.path().join("profile");
        write_command(&workspace, "same", "workspace");
        write_command(&profile, "same", "profile");
        #[cfg(unix)]
        std::os::unix::fs::symlink(workspace.join("same.md"), workspace.join("linked.md")).unwrap();
        let registry = PromptCommandRegistry::discover_roots(&[
            (PromptCommandScope::Workspace, workspace),
            (PromptCommandScope::ProfileAutoDistill, profile),
        ]);
        assert_eq!(registry.commands().len(), 1);
        assert_eq!(registry.commands()[0].body, "workspace");
    }

    #[test]
    fn every_workspace_root_contributes_commands_in_root_order() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary");
        let additional = temp.path().join("additional");
        fs::create_dir_all(&primary).unwrap();
        fs::create_dir_all(&additional).unwrap();
        write_command(&primary.join(".ca/commands"), "primary-only", "primary");
        write_command(
            &additional.join(".ca/commands"),
            "additional-only",
            "additional",
        );
        write_command(&primary.join(".ca/commands"), "same", "primary wins");
        write_command(&additional.join(".ca/commands"), "same", "additional loses");
        let primary = fs::canonicalize(primary).unwrap();
        let additional = fs::canonicalize(additional).unwrap();
        let workspace = crate::workspace::WorkspaceContext::from_roots(
            &primary,
            std::slice::from_ref(&additional),
        )
        .unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path().join("home"));
        let registry = PromptCommandRegistry::discover_for_workspace(
            &paths,
            &crate::runtime::ActiveProfile::Default,
            &workspace,
        );
        assert_eq!(registry.find("primary-only").unwrap().body, "primary");
        assert_eq!(registry.find("additional-only").unwrap().body, "additional");
        assert_eq!(registry.find("same").unwrap().body, "primary wins");
    }

    #[test]
    fn user_only_discovery_excludes_workspace_and_retains_profile_commands() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path().join("home"));
        let profile = crate::runtime::ActiveProfile::Default;
        write_command(&paths.home.join(".ca/commands"), "user-only", "user");
        write_command(
            &paths.auto_distilled_commands_dir(&profile),
            "profile-only",
            "profile",
        );
        write_command(
            &temp.path().join("workspace/.ca/commands"),
            "workspace-only",
            "workspace",
        );
        let registry = PromptCommandRegistry::discover_user_only(&paths, &profile);
        assert_eq!(registry.find("user-only").unwrap().body, "user");
        assert_eq!(registry.find("profile-only").unwrap().body, "profile");
        assert!(registry.find("workspace-only").is_none());
    }

    #[test]
    fn discovers_mimo_commands_as_inert_ordinary_prompt_turns() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let workspace = temp.path().join("workspace");
        let commands = temp.path().join(".mimocode/command");
        write_command(
            &commands,
            "check-rsync",
            "Inspect $1 and summarize the bounded status for $ARGUMENTS",
        );
        write_command(&commands, "help", "must not shadow CA help");

        let registry = PromptCommandRegistry::discover(&paths, &profile, &workspace);

        let command = registry.find("check-rsync").unwrap();
        assert_eq!(command.scope, PromptCommandScope::Compatibility);
        assert_eq!(
            registry
                .expand_line("/check-rsync bsd-primary --settled")
                .unwrap()
                .unwrap(),
            "Inspect bsd-primary and summarize the bounded status for bsd-primary --settled"
        );
        assert!(registry.find("help").is_none());
    }

    #[test]
    fn generated_commands_publish_replace_by_exact_digest_and_preserve_human_files() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::runtime::RuntimePaths::from_home(temp.path());
        let profile = crate::runtime::ActiveProfile::Default;
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(workspace.join(".ca/commands")).unwrap();
        write_command(
            &workspace.join(".ca/commands"),
            "human-check",
            "Keep this human prompt",
        );
        let source_ids = vec![uuid::Uuid::new_v4(), uuid::Uuid::new_v4()];
        let mut command = crate::learning::DistillCommandPackage {
            name: "check-bsd".into(),
            description: "Check a BSD transfer".into(),
            body: "Inspect $1 and summarize $ARGUMENTS".into(),
            agent: None,
            scope: crate::learning::ReusableAssetScope::Workspace,
            source_session_ids: source_ids.clone(),
            replaces_digest: None,
        };
        let human = crate::learning::DistillCommandPackage {
            name: "human-check".into(),
            description: "Try to replace a human command".into(),
            body: "Replacement".into(),
            agent: None,
            scope: crate::learning::ReusableAssetScope::Workspace,
            source_session_ids: source_ids,
            replaces_digest: None,
        };
        let first_plan = plan_auto_distilled_commands(
            &paths,
            &profile,
            &workspace,
            uuid::Uuid::new_v4(),
            &[command.clone(), human],
        )
        .unwrap();
        let first = publish_prompt_asset_plan(&first_plan).unwrap();
        assert_eq!(first.created.len(), 1);
        assert_eq!(first.conflicts.len(), 1);
        let target = workspace.join(".ca/commands/check-bsd.md");
        let registry = PromptCommandRegistry::discover(&paths, &profile, &workspace);
        assert_eq!(
            registry
                .expand_line("/check-bsd host extra")
                .unwrap()
                .unwrap(),
            "Inspect host and summarize host extra"
        );

        command.body = "Review $1 safely".into();
        command.replaces_digest = Some(content_digest(&fs::read(&target).unwrap()));
        let second_run = uuid::Uuid::new_v4();
        let second_plan =
            plan_auto_distilled_commands(&paths, &profile, &workspace, second_run, &[command])
                .unwrap();
        let second = publish_prompt_asset_plan(&second_plan).unwrap();
        assert_eq!(second.created, vec![target.clone()]);
        assert!(second.previous.contains_key(&target));
        assert!(fs::read_to_string(target)
            .unwrap()
            .contains("Review $1 safely"));
        assert!(
            fs::read_to_string(workspace.join(".ca/commands/human-check.md"))
                .unwrap()
                .contains("Keep this human prompt")
        );
    }

    #[test]
    fn unambiguous_skills_are_immediately_available_as_ordinary_slash_prompts() {
        let temp = tempfile::tempdir().unwrap();
        let skills_root = temp.path().join("skills");
        let skill = skills_root.join("check-transfer");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: check-transfer\ndescription: Check a transfer safely\n---\nInspect the transfer and stop when healthy.\n",
        )
        .unwrap();
        let skills = crate::skills::SkillRegistry::discover_portable(
            vec![crate::skills::SkillDiscoveryRoot::new(
                crate::skills::SkillSourceScope::Workspace,
                skills_root,
                "workspace",
            )],
            &[],
        );
        let registry = PromptCommandRegistry::default().include_skills(&skills);
        let expanded = registry
            .expand_line("/check-transfer bsd-primary")
            .unwrap()
            .unwrap();
        assert!(expanded.starts_with("$workspace::"));
        assert!(expanded.ends_with("bsd-primary"));
    }
}
