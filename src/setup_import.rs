//! Previewed, digest-bound import of inert Claude setup assets.
//!
//! This importer deliberately excludes settings, hooks, MCP configuration,
//! credentials, permissions, shell setup, and account state. Publication is a
//! no-overwrite transaction into CA-native roots.

use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const SCHEMA: &str = "ca.setup-import-preview.v1";
const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SetupImportScope {
    User,
    Project,
}

impl SetupImportScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupImportEntry {
    pub kind: String,
    pub source: String,
    pub target: String,
    pub source_sha256: String,
    pub target_sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupImportSkipped {
    pub source: String,
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupImportPreview {
    pub schema: String,
    pub source_root: String,
    pub target_root: String,
    pub scope: SetupImportScope,
    pub digest: String,
    pub files: Vec<SetupImportEntry>,
    pub skipped: Vec<SetupImportSkipped>,
    pub collisions: Vec<String>,
    pub total_source_bytes: u64,
    #[serde(skip)]
    writes: Vec<PlannedWrite>,
    #[serde(skip)]
    target_anchor: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedWrite {
    source: PathBuf,
    target: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupImportResult {
    pub schema: String,
    pub digest: String,
    pub imported_files: usize,
    pub imported_bytes: u64,
    pub target_root: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SetupImportError {
    #[error("Claude setup source must be a bounded regular directory")]
    InvalidSource,
    #[error("Claude setup contains a symlink, hardlink, or special file")]
    UnsafeSource,
    #[error("Claude setup exceeds the 256-file, 256-KiB-per-file, or 8-MiB total bound")]
    TooLarge,
    #[error("Claude setup contains malformed {0}")]
    Malformed(&'static str),
    #[error("Claude setup contains no importable instructions, rules, commands, or skills")]
    Empty,
    #[error("setup import destination is unsafe")]
    UnsafeDestination,
    #[error("setup import target already exists")]
    Collision,
    #[error("setup import preview changed; create a fresh preview")]
    Drift,
    #[error("the bare profile cannot apply setup imports")]
    BareProfile,
    #[error("setup import could not be completed")]
    Io,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca import",
    about = "Preview or apply inert local setup imports"
)]
pub(crate) struct SetupImportCliArgs {
    #[command(subcommand)]
    command: SetupImportCommand,
}

#[derive(Debug, Subcommand)]
enum SetupImportCommand {
    /// Import instructions, rules, commands, and skills from Claude setup
    Setup {
        #[command(subcommand)]
        source: SetupImportSource,
    },
}

#[derive(Debug, Subcommand)]
enum SetupImportSource {
    Claude {
        #[arg(long, value_name = "DIR")]
        path: PathBuf,
        #[arg(long, value_enum)]
        scope: SetupImportScope,
        #[arg(long)]
        json: bool,
        #[arg(long, value_name = "SHA256")]
        apply_digest: Option<String>,
    },
}

pub(crate) fn parse_args<I, T>(args: I) -> Result<SetupImportCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    SetupImportCliArgs::try_parse_from(args)
}

pub(crate) fn run_cli(cli: SetupImportCliArgs) -> i32 {
    let result = match cli.command {
        SetupImportCommand::Setup {
            source:
                SetupImportSource::Claude {
                    path,
                    scope,
                    json,
                    apply_digest,
                },
        } => run_claude_cli(path, scope, json, apply_digest.as_deref()),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Setup import error: {error}");
            2
        }
    }
}

fn run_claude_cli(
    path: PathBuf,
    scope: SetupImportScope,
    json: bool,
    apply_digest: Option<&str>,
) -> Result<(), SetupImportError> {
    let paths = crate::runtime::RuntimePaths::discover().map_err(|_| SetupImportError::Io)?;
    let workspace = std::env::current_dir().map_err(|_| SetupImportError::Io)?;
    let preview = preview_claude(&path, scope, &paths.home, &workspace)?;
    if let Some(digest) = apply_digest {
        let result = apply_claude(
            &path,
            scope,
            &paths.home,
            &workspace,
            &crate::runtime::ActiveProfile::Default,
            digest,
        )?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).map_err(|_| SetupImportError::Io)?
            );
        } else {
            println!(
                "Imported {} Claude setup files ({} bytes) into {}",
                result.imported_files, result.imported_bytes, result.target_root
            );
        }
    } else if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&preview).map_err(|_| SetupImportError::Io)?
        );
    } else {
        println!("{}", render_preview(&preview));
        println!(
            "\nNo changes made. Re-run with --apply-digest {}",
            preview.digest
        );
    }
    Ok(())
}

pub(crate) fn preview_claude(
    source: &Path,
    scope: SetupImportScope,
    home: &Path,
    workspace: &Path,
) -> Result<SetupImportPreview, SetupImportError> {
    let source_meta = fs::symlink_metadata(source).map_err(|_| SetupImportError::InvalidSource)?;
    if !source_meta.is_dir() || source_meta.file_type().is_symlink() {
        return Err(SetupImportError::InvalidSource);
    }
    let source = fs::canonicalize(source).map_err(|_| SetupImportError::InvalidSource)?;
    let home = canonical_directory(home)?;
    let workspace = canonical_directory(workspace)?;
    let (target_anchor, target_root) = match scope {
        SetupImportScope::User => (home.clone(), home.join(".ca")),
        SetupImportScope::Project => (workspace.clone(), workspace.join(".ca")),
    };
    validate_destination_ancestors(&target_anchor, &target_root)?;

    let inventory = inventory_source(&source)?;
    let mut writes = Vec::new();
    let mut skipped = Vec::new();

    if let Some(file) = inventory.get(Path::new("CLAUDE.md")) {
        let body = read_utf8(file)?;
        let content = crate::rules::render_rule_document(
            "Imported Claude project instructions",
            crate::rules::RuleActivation::Always,
            Vec::new(),
            &body,
        )
        .map_err(|_| SetupImportError::Malformed("CLAUDE.md"))?;
        writes.push(PlannedWrite {
            source: file.clone(),
            target: target_root.join("rules/claude/CLAUDE.md"),
            bytes: content.into_bytes(),
        });
    }

    for (relative, file) in inventory.iter().filter(|(relative, _)| {
        relative.starts_with(".claude/rules") && relative.extension().is_some_and(|ext| ext == "md")
    }) {
        let inside = relative
            .strip_prefix(".claude/rules")
            .map_err(|_| SetupImportError::UnsafeSource)?;
        validate_relative(inside)?;
        let source_text = read_utf8(file)?;
        let (description, globs, body) = convert_claude_rule(&source_text, inside)?;
        let activation = if globs.is_empty() {
            crate::rules::RuleActivation::Always
        } else {
            crate::rules::RuleActivation::Glob
        };
        let content = crate::rules::render_rule_document(&description, activation, globs, &body)
            .map_err(|_| SetupImportError::Malformed("rule frontmatter"))?;
        writes.push(PlannedWrite {
            source: file.clone(),
            target: target_root.join("rules/claude").join(inside),
            bytes: content.into_bytes(),
        });
    }

    for (relative, file) in inventory.iter().filter(|(relative, _)| {
        relative.starts_with(".claude/commands")
            && relative.extension().is_some_and(|ext| ext == "md")
    }) {
        let inside = relative
            .strip_prefix(".claude/commands")
            .map_err(|_| SetupImportError::UnsafeSource)?;
        validate_relative(inside)?;
        let name = inside
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or(SetupImportError::Malformed("command name"))?
            .to_ascii_lowercase();
        crate::prompt_commands::validate_name(&name)
            .map_err(|_| SetupImportError::Malformed("command name"))?;
        if crate::tui::commands::find_command(&name).is_some() {
            return Err(SetupImportError::Malformed("command name"));
        }
        let source_text = read_utf8(file)?;
        let (description, body) = convert_claude_command(&source_text, &name)?;
        let content = format!(
            "---\ndescription: {}\n---\n\n{}\n",
            yaml_scalar(&description),
            body.trim()
        );
        if content.len() > 64 * 1024
            || crate::learning::sanitized_learning_text(&body).as_deref() != Some(body.trim())
        {
            return Err(SetupImportError::Malformed("command"));
        }
        writes.push(PlannedWrite {
            source: file.clone(),
            target: target_root.join("commands").join(inside),
            bytes: content.into_bytes(),
        });
    }

    let skills_root = source.join(".claude/skills");
    if skills_root.is_dir() {
        plan_skills(&skills_root, &target_root, &inventory, &mut writes)?;
    }

    for (relative, file) in &inventory {
        if writes.iter().any(|write| write.source == *file) {
            continue;
        }
        let (category, reason) = skipped_reason(relative);
        skipped.push(SetupImportSkipped {
            source: relative.to_string_lossy().into_owned(),
            category: category.into(),
            reason: reason.into(),
        });
    }
    skipped.sort_by(|left, right| left.source.cmp(&right.source));

    writes.sort_by(|left, right| left.target.cmp(&right.target));
    if writes.is_empty() {
        return Err(SetupImportError::Empty);
    }
    let mut targets = BTreeSet::new();
    if writes
        .iter()
        .any(|write| !targets.insert(write.target.clone()))
    {
        return Err(SetupImportError::Collision);
    }
    let collisions = writes
        .iter()
        .filter(|write| fs::symlink_metadata(&write.target).is_ok())
        .map(|write| write.target.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let files = writes
        .iter()
        .map(|write| {
            Ok(SetupImportEntry {
                kind: target_kind(&write.target).into(),
                source: write.source.to_string_lossy().into_owned(),
                target: write.target.to_string_lossy().into_owned(),
                source_sha256: digest_bytes(
                    &fs::read(&write.source).map_err(|_| SetupImportError::Io)?,
                ),
                target_sha256: digest_bytes(&write.bytes),
                bytes: write.bytes.len() as u64,
            })
        })
        .collect::<Result<Vec<_>, SetupImportError>>()?;
    let total_source_bytes = inventory
        .values()
        .filter_map(|path| fs::metadata(path).ok().map(|metadata| metadata.len()))
        .sum();
    let mut preview = SetupImportPreview {
        schema: SCHEMA.into(),
        source_root: source.to_string_lossy().into_owned(),
        target_root: target_root.to_string_lossy().into_owned(),
        scope,
        digest: String::new(),
        files,
        skipped,
        collisions,
        total_source_bytes,
        writes,
        target_anchor,
    };
    preview.digest = preview_digest(&preview)?;
    Ok(preview)
}

pub(crate) fn apply_claude(
    source: &Path,
    scope: SetupImportScope,
    home: &Path,
    workspace: &Path,
    profile: &crate::runtime::ActiveProfile,
    expected_digest: &str,
) -> Result<SetupImportResult, SetupImportError> {
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err(SetupImportError::BareProfile);
    }
    if !is_sha256(expected_digest) {
        return Err(SetupImportError::Drift);
    }
    let preview = preview_claude(source, scope, home, workspace)?;
    if preview.digest != expected_digest {
        return Err(SetupImportError::Drift);
    }
    apply_preview(&preview, None)
}

fn apply_preview(
    preview: &SetupImportPreview,
    #[cfg_attr(not(test), allow(unused_variables))] fail_after: Option<usize>,
) -> Result<SetupImportResult, SetupImportError> {
    if !preview.collisions.is_empty() {
        return Err(SetupImportError::Collision);
    }
    validate_destination_ancestors(&preview.target_anchor, Path::new(&preview.target_root))?;
    for write in &preview.writes {
        if fs::symlink_metadata(&write.target).is_ok() {
            return Err(SetupImportError::Drift);
        }
    }
    let staging = preview
        .target_anchor
        .join(format!(".ca-setup-import-{}.tmp", uuid::Uuid::new_v4()));
    create_private_dir(&staging)?;
    let staged_result = (|| {
        for (index, write) in preview.writes.iter().enumerate() {
            let staged = staging.join(index.to_string());
            write_private_file(&staged, &write.bytes)?;
        }
        sync_directory(&staging)?;
        Ok(())
    })();
    if let Err(error) = staged_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let mut published = Vec::new();
    let mut created_directories = Vec::new();
    let publication = (|| {
        for (index, write) in preview.writes.iter().enumerate() {
            #[cfg(test)]
            if fail_after == Some(index) {
                return Err(SetupImportError::Io);
            }
            ensure_destination_parent(
                &preview.target_anchor,
                write
                    .target
                    .parent()
                    .ok_or(SetupImportError::UnsafeDestination)?,
                &mut created_directories,
            )?;
            let staged = staging.join(index.to_string());
            fs::hard_link(&staged, &write.target).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SetupImportError::Drift
                } else {
                    SetupImportError::Io
                }
            })?;
            published.push(write.target.clone());
            fs::remove_file(&staged).map_err(|_| SetupImportError::Io)?;
            sync_directory(
                write
                    .target
                    .parent()
                    .ok_or(SetupImportError::UnsafeDestination)?,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = publication {
        for path in published.iter().rev() {
            let _ = fs::remove_file(path);
        }
        for path in created_directories.iter().rev() {
            let _ = fs::remove_dir(path);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    fs::remove_dir(&staging).map_err(|_| SetupImportError::Io)?;
    sync_directory(&preview.target_anchor)?;
    Ok(SetupImportResult {
        schema: SCHEMA.into(),
        digest: preview.digest.clone(),
        imported_files: preview.writes.len(),
        imported_bytes: preview
            .writes
            .iter()
            .map(|write| write.bytes.len() as u64)
            .sum(),
        target_root: preview.target_root.clone(),
    })
}

pub(crate) fn render_preview(preview: &SetupImportPreview) -> String {
    let mut output = format!(
        "Claude setup import preview\n\nScope: {}\nSource: {}\nTarget: {}\nDigest: {}\nFiles: {}\nInput bytes: {}",
        preview.scope.label(),
        preview.source_root,
        preview.target_root,
        preview.digest,
        preview.files.len(),
        preview.total_source_bytes
    );
    for file in &preview.files {
        output.push_str(&format!(
            "\n- {}: {} -> {} ({})",
            file.kind, file.source, file.target, file.target_sha256
        ));
    }
    if !preview.skipped.is_empty() {
        output.push_str("\n\nSkipped (not read or imported):");
        for skipped in &preview.skipped {
            output.push_str(&format!(
                "\n- {} [{}]: {}",
                skipped.source, skipped.category, skipped.reason
            ));
        }
    }
    if !preview.collisions.is_empty() {
        output.push_str("\n\nBlocked collisions:");
        for collision in &preview.collisions {
            output.push_str(&format!("\n- {collision}"));
        }
    }
    output
}

fn inventory_source(source: &Path) -> Result<BTreeMap<PathBuf, PathBuf>, SetupImportError> {
    let mut inventory = BTreeMap::new();
    let mut total = 0_u64;
    inventory_directory(source, source, 0, &mut inventory, &mut total)?;
    Ok(inventory)
}

fn inventory_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    inventory: &mut BTreeMap<PathBuf, PathBuf>,
    total: &mut u64,
) -> Result<(), SetupImportError> {
    if depth > MAX_DEPTH {
        return Err(SetupImportError::TooLarge);
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|_| SetupImportError::Io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SetupImportError::Io)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| SetupImportError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(SetupImportError::UnsafeSource);
        }
        if metadata.is_dir() {
            inventory_directory(root, &path, depth + 1, inventory, total)?;
            continue;
        }
        if !metadata.is_file() || hardlink_count(&metadata) > 1 {
            return Err(SetupImportError::UnsafeSource);
        }
        if metadata.len() > MAX_FILE_BYTES || inventory.len() >= MAX_FILES {
            return Err(SetupImportError::TooLarge);
        }
        *total = total
            .checked_add(metadata.len())
            .ok_or(SetupImportError::TooLarge)?;
        if *total > MAX_TOTAL_BYTES {
            return Err(SetupImportError::TooLarge);
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| SetupImportError::UnsafeSource)?
            .to_path_buf();
        validate_relative(&relative)?;
        inventory.insert(relative, path);
    }
    Ok(())
}

fn plan_skills(
    skills_root: &Path,
    target_root: &Path,
    inventory: &BTreeMap<PathBuf, PathBuf>,
    writes: &mut Vec<PlannedWrite>,
) -> Result<(), SetupImportError> {
    let registry = crate::skills::SkillRegistry::discover_portable(
        vec![crate::skills::SkillDiscoveryRoot::new(
            crate::skills::SkillSourceScope::Legacy,
            skills_root,
            "claude-import",
        )],
        &[],
    );
    if !registry.diagnostics().is_empty() {
        return Err(SetupImportError::Malformed("skill package"));
    }
    let mut package_roots = BTreeSet::new();
    for instance in registry.instances() {
        let relative_package = instance
            .locator
            .strip_prefix("claude-import/")
            .map(PathBuf::from)
            .ok_or(SetupImportError::Malformed("skill package"))?;
        validate_relative(&relative_package)?;
        let package = skills_root.join(&relative_package);
        package_roots.insert(relative_package.clone());
        for file in &instance.files {
            let source = package.join(&file.relative_path);
            let bytes = fs::read(&source).map_err(|_| SetupImportError::Io)?;
            if digest_bytes(&bytes) != file.sha256 {
                return Err(SetupImportError::Drift);
            }
            writes.push(PlannedWrite {
                source,
                target: target_root
                    .join("skills")
                    .join(&relative_package)
                    .join(&file.relative_path),
                bytes,
            });
        }
    }
    for relative in inventory
        .keys()
        .filter(|relative| relative.starts_with(".claude/skills"))
    {
        let inside = relative
            .strip_prefix(".claude/skills")
            .map_err(|_| SetupImportError::UnsafeSource)?;
        if inside.as_os_str().is_empty() {
            continue;
        }
        let belongs = package_roots
            .iter()
            .any(|package| inside.starts_with(package));
        if !belongs {
            return Err(SetupImportError::Malformed("skill package"));
        }
    }
    Ok(())
}

fn convert_claude_rule(
    source: &str,
    relative: &Path,
) -> Result<(String, Vec<String>, String), SetupImportError> {
    let (frontmatter, body) = split_optional_frontmatter(source, "rule frontmatter")?;
    let mut description = format!(
        "Imported Claude rule {}",
        relative
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("rule")
    );
    let mut globs = Vec::new();
    if let Some(value) = frontmatter {
        let mapping = value
            .as_mapping()
            .ok_or(SetupImportError::Malformed("rule frontmatter"))?;
        for (key, value) in mapping {
            let key = key
                .as_str()
                .ok_or(SetupImportError::Malformed("rule frontmatter"))?;
            match key {
                "description" => {
                    description = value
                        .as_str()
                        .ok_or(SetupImportError::Malformed("rule frontmatter"))?
                        .to_string();
                }
                "paths" | "globs" => globs.extend(yaml_string_list(value)?),
                _ => {}
            }
        }
    }
    Ok((description, globs, body.trim().to_string()))
}

fn convert_claude_command(source: &str, name: &str) -> Result<(String, String), SetupImportError> {
    let (frontmatter, body) = split_optional_frontmatter(source, "command frontmatter")?;
    let mut description = format!("Imported Claude command {name}");
    if let Some(value) = frontmatter {
        let mapping = value
            .as_mapping()
            .ok_or(SetupImportError::Malformed("command frontmatter"))?;
        if let Some(value) = mapping.get(serde_yaml_ng::Value::String("description".into())) {
            description = value
                .as_str()
                .ok_or(SetupImportError::Malformed("command frontmatter"))?
                .to_string();
        }
    }
    if description.is_empty() || description.len() > 240 || body.trim().is_empty() {
        return Err(SetupImportError::Malformed("command"));
    }
    Ok((description, body.trim().to_string()))
}

fn split_optional_frontmatter(
    source: &str,
    kind: &'static str,
) -> Result<(Option<serde_yaml_ng::Value>, String), SetupImportError> {
    let normalized = source.replace("\r\n", "\n");
    if !normalized.starts_with("---\n") {
        if normalized.trim().is_empty() {
            return Err(SetupImportError::Malformed(kind));
        }
        return Ok((None, normalized));
    }
    let remainder = &normalized[4..];
    let end = remainder
        .find("\n---\n")
        .ok_or(SetupImportError::Malformed(kind))?;
    let value = serde_yaml_ng::from_str(&remainder[..end])
        .map_err(|_| SetupImportError::Malformed(kind))?;
    let body = remainder[end + 5..].to_string();
    if body.trim().is_empty() {
        return Err(SetupImportError::Malformed(kind));
    }
    Ok((Some(value), body))
}

fn yaml_string_list(value: &serde_yaml_ng::Value) -> Result<Vec<String>, SetupImportError> {
    if let Some(value) = value.as_str() {
        return Ok(value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect());
    }
    value
        .as_sequence()
        .ok_or(SetupImportError::Malformed("rule frontmatter"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or(SetupImportError::Malformed("rule frontmatter"))
        })
        .collect()
}

fn yaml_scalar(value: &str) -> String {
    serde_yaml_ng::to_string(value)
        .unwrap_or_else(|_| "Imported Claude command\n".into())
        .trim()
        .to_string()
}

fn preview_digest(preview: &SetupImportPreview) -> Result<String, SetupImportError> {
    #[derive(Serialize)]
    struct Bound<'a> {
        schema: &'a str,
        source_root: &'a str,
        target_root: &'a str,
        scope: SetupImportScope,
        files: &'a [SetupImportEntry],
        skipped: &'a [SetupImportSkipped],
        collisions: &'a [String],
        total_source_bytes: u64,
    }
    let bytes = serde_json::to_vec(&Bound {
        schema: &preview.schema,
        source_root: &preview.source_root,
        target_root: &preview.target_root,
        scope: preview.scope,
        files: &preview.files,
        skipped: &preview.skipped,
        collisions: &preview.collisions,
        total_source_bytes: preview.total_source_bytes,
    })
    .map_err(|_| SetupImportError::Io)?;
    Ok(digest_bytes(&bytes))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, SetupImportError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| SetupImportError::UnsafeDestination)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SetupImportError::UnsafeDestination);
    }
    fs::canonicalize(path).map_err(|_| SetupImportError::UnsafeDestination)
}

fn validate_destination_ancestors(anchor: &Path, target: &Path) -> Result<(), SetupImportError> {
    if !target.starts_with(anchor) {
        return Err(SetupImportError::UnsafeDestination);
    }
    let relative = target
        .strip_prefix(anchor)
        .map_err(|_| SetupImportError::UnsafeDestination)?;
    let mut current = anchor.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(SetupImportError::UnsafeDestination);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(SetupImportError::UnsafeDestination),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(SetupImportError::UnsafeDestination),
        }
    }
    Ok(())
}

fn ensure_destination_parent(
    anchor: &Path,
    parent: &Path,
    created: &mut Vec<PathBuf>,
) -> Result<(), SetupImportError> {
    validate_destination_ancestors(anchor, parent)?;
    let relative = parent
        .strip_prefix(anchor)
        .map_err(|_| SetupImportError::UnsafeDestination)?;
    let mut current = anchor.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(SetupImportError::UnsafeDestination);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(SetupImportError::UnsafeDestination),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_private_dir(&current)?;
                created.push(current.clone());
            }
            Err(_) => return Err(SetupImportError::UnsafeDestination),
        }
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<(), SetupImportError> {
    fs::create_dir(path).map_err(|_| SetupImportError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| SetupImportError::Io)?;
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), SetupImportError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| SetupImportError::Io)?;
    file.write_all(bytes).map_err(|_| SetupImportError::Io)?;
    file.sync_all().map_err(|_| SetupImportError::Io)
}

fn sync_directory(path: &Path) -> Result<(), SetupImportError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| SetupImportError::Io)
}

fn validate_relative(path: &Path) -> Result<(), SetupImportError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SetupImportError::UnsafeSource);
    }
    Ok(())
}

fn read_utf8(path: &Path) -> Result<String, SetupImportError> {
    let bytes = fs::read(path).map_err(|_| SetupImportError::Io)?;
    String::from_utf8(bytes).map_err(|_| SetupImportError::Malformed("UTF-8 input"))
}

fn skipped_reason(relative: &Path) -> (&'static str, &'static str) {
    let text = relative.to_string_lossy().to_ascii_lowercase();
    if text.contains("credential") || text.contains("account") || text.contains("auth") {
        (
            "credentials",
            "credential and account metadata is never imported",
        )
    } else if text.contains("hook") {
        ("hooks", "executable hooks are never imported")
    } else if text.contains("mcp") {
        ("mcp", "MCP configuration is never imported")
    } else if text.contains("permission") {
        ("permissions", "permission settings are never imported")
    } else if text.contains("shell") || text.ends_with(".env") {
        (
            "shell",
            "shell initialization and environment values are never imported",
        )
    } else if text.contains("setting") || text.ends_with(".json") {
        ("settings", "Claude settings are never imported")
    } else {
        (
            "unsupported",
            "file is outside the supported inert setup formats",
        )
    }
}

fn target_kind(path: &Path) -> &'static str {
    let text = path.to_string_lossy();
    if text.contains("/rules/") {
        "rule"
    } else if text.contains("/commands/") {
        "command"
    } else {
        "skill"
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(unix)]
fn hardlink_count(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

#[cfg(not(unix))]
fn hardlink_count(_metadata: &fs::Metadata) -> u64 {
    1
}

#[cfg(test)]
pub(crate) fn test_preview() -> SetupImportPreview {
    SetupImportPreview {
        schema: SCHEMA.into(),
        source_root: "/workspace/claude-setup".into(),
        target_root: "/workspace/.ca".into(),
        scope: SetupImportScope::Project,
        digest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        files: vec![
            SetupImportEntry {
                kind: "rule".into(),
                source: "/workspace/claude-setup/CLAUDE.md".into(),
                target: "/workspace/.ca/rules/claude/CLAUDE.md".into(),
                source_sha256: "1111111111111111111111111111111111111111111111111111111111111111"
                    .into(),
                target_sha256: "2222222222222222222222222222222222222222222222222222222222222222"
                    .into(),
                bytes: 240,
            },
            SetupImportEntry {
                kind: "skill".into(),
                source: "/workspace/claude-setup/.claude/skills/review/SKILL.md".into(),
                target: "/workspace/.ca/skills/review/SKILL.md".into(),
                source_sha256: "3333333333333333333333333333333333333333333333333333333333333333"
                    .into(),
                target_sha256: "3333333333333333333333333333333333333333333333333333333333333333"
                    .into(),
                bytes: 480,
            },
        ],
        skipped: vec![SetupImportSkipped {
            source: ".claude/settings.json".into(),
            category: "settings".into(),
            reason: "Claude settings are never imported".into(),
        }],
        collisions: Vec::new(),
        total_source_bytes: 777,
        writes: Vec::new(),
        target_anchor: PathBuf::from("/workspace"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let home = temp.path().join("home");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(source.join(".claude/rules")).unwrap();
        fs::create_dir_all(source.join(".claude/commands")).unwrap();
        fs::create_dir_all(source.join(".claude/skills/deploy")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(source.join("CLAUDE.md"), "Keep changes focused.\n").unwrap();
        fs::write(
            source.join(".claude/rules/rust.md"),
            "---\ndescription: Rust files\npaths: [\"**/*.rs\"]\n---\nUse rustfmt.\n",
        )
        .unwrap();
        fs::write(
            source.join(".claude/commands/check.md"),
            "---\ndescription: Check locally\nallowed-tools: Bash\n---\nRun the local checks.\n",
        )
        .unwrap();
        fs::write(
            source.join(".claude/skills/deploy/SKILL.md"),
            "---\nname: deploy\ndescription: Deploy safely\n---\nFollow the release checklist.\n",
        )
        .unwrap();
        fs::write(source.join(".claude/settings.json"), "SECRET_SENTINEL").unwrap();
        (temp, source, home, workspace)
    }

    #[test]
    fn preview_maps_only_inert_assets_and_never_retains_sensitive_values() {
        let (_temp, source, home, workspace) = fixture();
        let preview =
            preview_claude(&source, SetupImportScope::Project, &home, &workspace).unwrap();
        assert_eq!(preview.schema, SCHEMA);
        assert_eq!(preview.files.len(), 4);
        assert_eq!(preview.skipped.len(), 1);
        assert_eq!(preview.skipped[0].category, "settings");
        let json = serde_json::to_string(&preview).unwrap();
        assert!(!json.contains("SECRET_SENTINEL"));
        assert!(preview.files.iter().any(|entry| entry.kind == "skill"));
        assert!(preview.files.iter().any(|entry| entry.kind == "rule"));
        assert!(preview.files.iter().any(|entry| entry.kind == "command"));
        assert!(is_sha256(&preview.digest));
    }

    #[test]
    fn exact_digest_apply_is_atomic_native_and_refuses_drift_or_collisions() {
        let (_temp, source, home, workspace) = fixture();
        let preview =
            preview_claude(&source, SetupImportScope::Project, &home, &workspace).unwrap();
        let result = apply_claude(
            &source,
            SetupImportScope::Project,
            &home,
            &workspace,
            &crate::runtime::ActiveProfile::Default,
            &preview.digest,
        )
        .unwrap();
        assert_eq!(result.imported_files, 4);
        assert!(workspace.join(".ca/rules/claude/CLAUDE.md").is_file());
        assert!(workspace.join(".ca/rules/claude/rust.md").is_file());
        assert!(workspace.join(".ca/commands/check.md").is_file());
        assert!(workspace.join(".ca/skills/deploy/SKILL.md").is_file());
        assert_eq!(
            apply_claude(
                &source,
                SetupImportScope::Project,
                &home,
                &workspace,
                &crate::runtime::ActiveProfile::Default,
                &preview.digest,
            ),
            Err(SetupImportError::Drift)
        );
        fs::write(source.join("CLAUDE.md"), "changed\n").unwrap();
        assert_eq!(
            apply_claude(
                &source,
                SetupImportScope::User,
                &home,
                &workspace,
                &crate::runtime::ActiveProfile::Default,
                &preview.digest,
            ),
            Err(SetupImportError::Drift)
        );
    }

    #[test]
    fn user_scope_and_collision_preview_are_exact_and_no_overwrite() {
        let (_temp, source, home, workspace) = fixture();
        let preview = preview_claude(&source, SetupImportScope::User, &home, &workspace).unwrap();
        let canonical_home = fs::canonicalize(&home).unwrap();
        assert_eq!(
            preview.target_root,
            canonical_home.join(".ca").to_string_lossy()
        );
        assert!(preview
            .files
            .iter()
            .all(|entry| Path::new(&entry.target).starts_with(canonical_home.join(".ca"))));
        let collision = canonical_home.join(".ca/rules/claude/CLAUDE.md");
        fs::create_dir_all(collision.parent().unwrap()).unwrap();
        fs::write(&collision, "owned by user\n").unwrap();
        let blocked = preview_claude(&source, SetupImportScope::User, &home, &workspace).unwrap();
        assert_eq!(blocked.collisions, vec![collision.to_string_lossy()]);
        assert_eq!(
            apply_claude(
                &source,
                SetupImportScope::User,
                &home,
                &workspace,
                &crate::runtime::ActiveProfile::Default,
                &blocked.digest,
            ),
            Err(SetupImportError::Collision)
        );
        assert_eq!(fs::read_to_string(collision).unwrap(), "owned by user\n");
    }

    #[test]
    fn publication_failure_rolls_back_every_owned_write() {
        let (_temp, source, home, workspace) = fixture();
        let preview =
            preview_claude(&source, SetupImportScope::Project, &home, &workspace).unwrap();
        assert_eq!(apply_preview(&preview, Some(2)), Err(SetupImportError::Io));
        for entry in &preview.files {
            assert!(!Path::new(&entry.target).exists());
        }
        assert!(!workspace
            .read_dir()
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("setup-import")));
    }

    #[test]
    fn symlink_hardlink_malformed_and_bare_inputs_fail_closed() {
        let (_temp, source, home, workspace) = fixture();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                source.join("CLAUDE.md"),
                source.join(".claude/rules/link.md"),
            )
            .unwrap();
            assert_eq!(
                preview_claude(&source, SetupImportScope::Project, &home, &workspace),
                Err(SetupImportError::UnsafeSource)
            );
            fs::remove_file(source.join(".claude/rules/link.md")).unwrap();
            fs::hard_link(
                source.join("CLAUDE.md"),
                source.join(".claude/rules/hard.md"),
            )
            .unwrap();
            assert_eq!(
                preview_claude(&source, SetupImportScope::Project, &home, &workspace),
                Err(SetupImportError::UnsafeSource)
            );
            fs::remove_file(source.join(".claude/rules/hard.md")).unwrap();
        }
        fs::write(source.join(".claude/rules/rust.md"), "---\nbad: [\n").unwrap();
        assert_eq!(
            preview_claude(&source, SetupImportScope::Project, &home, &workspace),
            Err(SetupImportError::Malformed("rule frontmatter"))
        );
        fs::write(source.join(".claude/rules/rust.md"), "Use rustfmt.\n").unwrap();
        let preview =
            preview_claude(&source, SetupImportScope::Project, &home, &workspace).unwrap();
        assert_eq!(
            apply_claude(
                &source,
                SetupImportScope::Project,
                &home,
                &workspace,
                &crate::runtime::ActiveProfile::Bare,
                &preview.digest,
            ),
            Err(SetupImportError::BareProfile)
        );
    }

    #[test]
    fn file_count_size_skill_manifest_and_destination_symlinks_are_bounded() {
        let (_temp, source, home, workspace) = fixture();
        fs::write(
            source.join(".claude/skills/deploy/SKILL.md"),
            "---\nname: deploy\ndescription: Deploy safely\nunknown: value\n---\nBody\n",
        )
        .unwrap();
        assert_eq!(
            preview_claude(&source, SetupImportScope::Project, &home, &workspace),
            Err(SetupImportError::Malformed("skill package"))
        );
        fs::write(
            source.join(".claude/skills/deploy/SKILL.md"),
            "---\nname: deploy\ndescription: Deploy safely\n---\nBody\n",
        )
        .unwrap();
        let oversized = source.join(".claude/settings-large.json");
        fs::write(&oversized, vec![b'x'; MAX_FILE_BYTES as usize + 1]).unwrap();
        assert_eq!(
            preview_claude(&source, SetupImportScope::Project, &home, &workspace),
            Err(SetupImportError::TooLarge)
        );
        fs::remove_file(oversized).unwrap();

        let ignored = source.join(".claude/ignored");
        fs::create_dir_all(&ignored).unwrap();
        for index in 0..MAX_FILES {
            fs::write(ignored.join(format!("{index}.txt")), b"x").unwrap();
        }
        assert_eq!(
            preview_claude(&source, SetupImportScope::Project, &home, &workspace),
            Err(SetupImportError::TooLarge)
        );
        fs::remove_dir_all(ignored).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(source.join(".claude"), workspace.join(".ca")).unwrap();
            assert_eq!(
                preview_claude(&source, SetupImportScope::Project, &home, &workspace),
                Err(SetupImportError::UnsafeDestination)
            );
        }
    }

    #[test]
    fn parser_requires_the_locked_command_shape() {
        let parsed = parse_args([
            "ca import",
            "setup",
            "claude",
            "--path",
            "/tmp/project",
            "--scope",
            "project",
            "--json",
        ])
        .unwrap();
        let SetupImportCommand::Setup {
            source: SetupImportSource::Claude { scope, json, .. },
        } = parsed.command;
        assert_eq!(scope, SetupImportScope::Project);
        assert!(json);
    }
}
