use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "ca skill", about = "Inspect and manage portable Agent Skills")]
pub struct SkillCliArgs {
    #[arg(long, default_value = "default")]
    profile: String,
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    List,
    Show {
        selector: String,
    },
    Enable {
        instance_id: String,
        digest: String,
    },
    Disable {
        instance_id: String,
        digest: String,
    },
    Install {
        source: PathBuf,
        #[arg(long)]
        confirm: Option<String>,
    },
    Import {
        source: PathBuf,
        #[arg(long)]
        confirm: Option<String>,
    },
    New {
        name: String,
        destination: PathBuf,
        #[arg(long)]
        confirm: Option<String>,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ExactState {
    #[serde(default)]
    records: BTreeMap<String, ExactRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExactRecord {
    digest: String,
    enabled: bool,
}

pub fn parse_args<I, T>(args: I) -> Result<SkillCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    SkillCliArgs::try_parse_from(args)
}

pub fn run_cli(cli: SkillCliArgs) -> i32 {
    match run(cli) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Skill error: {error}");
            2
        }
    }
}

fn run(cli: SkillCliArgs) -> Result<(), String> {
    let profile = crate::runtime::ActiveProfile::parse(Some(&cli.profile))
        .map_err(|error| error.to_string())?;
    if matches!(profile, crate::runtime::ActiveProfile::Bare) {
        return Err("the bare profile cannot manage skills".into());
    }
    let base = crate::runtime::RuntimePaths::discover().map_err(|error| error.to_string())?;
    let paths = base.clone().for_profile(&profile);
    let config = crate::config::ConfigStore::for_process_only_load(
        paths.config_file.clone(),
        profile.clone(),
    )
    .load()
    .map_err(|error| error.to_string())?;
    let workspace = crate::workspace::WorkspaceContext::discover(
        &std::env::current_dir().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut registry = crate::discover_skill_registry(&paths, &workspace, &config);
    let state_path = base.skill_state_file(&profile);
    let state = read_state(&state_path)?;
    for (id, record) in &state.records {
        let _ = registry.set_instance_enabled(id, &record.digest, record.enabled);
    }

    match cli.command {
        SkillCommand::List => {
            for instance in registry.instances() {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    instance.instance_id,
                    instance.qualified_name,
                    instance.content_digest,
                    if instance.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    instance.metadata.description
                );
            }
        }
        SkillCommand::Show { selector } => {
            let instance = registry
                .resolve_instance(&selector, true)
                .or_else(|_| {
                    registry
                        .instances()
                        .find(|instance| instance.instance_id == selector)
                        .ok_or_else(|| crate::skills::SkillError::Unknown(selector.clone()))
                })
                .map_err(|error| error.to_string())?;
            println!("instance_id: {}", instance.instance_id);
            println!("qualified_name: {}", instance.qualified_name);
            println!("scope: {}", instance.source_scope);
            println!("locator: {}", instance.locator);
            println!("digest: {}", instance.content_digest);
            println!("enabled: {}", instance.enabled);
            println!(
                "allowed_tools (inert): {}",
                instance.metadata.allowed_tools.as_deref().unwrap_or("none")
            );
            for dependency in &instance.metadata.dependencies {
                println!(
                    "dependency: {} {}",
                    dependency.dependency_type, dependency.value
                );
            }
            for file in &instance.files {
                println!(
                    "file: {} {} {}",
                    file.relative_path.display(),
                    file.size,
                    file.sha256
                );
            }
        }
        SkillCommand::Enable {
            instance_id,
            digest,
        } => {
            write_exact_state(&state_path, state, &instance_id, &digest, true, &registry)?;
            println!("Enabled exact skill {instance_id}");
        }
        SkillCommand::Disable {
            instance_id,
            digest,
        } => {
            write_exact_state(&state_path, state, &instance_id, &digest, false, &registry)?;
            println!("Disabled exact skill {instance_id}");
        }
        SkillCommand::Install { source, confirm } | SkillCommand::Import { source, confirm } => {
            install_local(
                &base.managed_skills_dir(&profile),
                &state_path,
                &source,
                confirm.as_deref(),
            )?;
        }
        SkillCommand::New {
            name,
            destination,
            confirm,
        } => {
            new_skill(&name, &destination, confirm.as_deref())?;
        }
    }
    Ok(())
}

fn read_state(path: &Path) -> Result<ExactState, String> {
    if !path.exists() {
        return Ok(ExactState::default());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err("skill state must be a bounded regular file".into());
    }
    serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

fn write_exact_state(
    path: &Path,
    mut state: ExactState,
    instance_id: &str,
    digest: &str,
    enabled: bool,
    registry: &crate::skills::SkillRegistry,
) -> Result<(), String> {
    crate::skills::resolve_pinned_instance(registry.instances(), instance_id, digest)
        .map_err(|error| error.to_string())?;
    state.records.insert(
        instance_id.to_string(),
        ExactRecord {
            digest: digest.to_string(),
            enabled,
        },
    );
    atomic_private_json(path, &state)
}

fn atomic_private_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("skill state path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let temporary = parent.join(format!(".state-{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn install_local(
    destination_root: &Path,
    state_path: &Path,
    source: &Path,
    confirm: Option<&str>,
) -> Result<(), String> {
    let source = std::fs::canonicalize(source).map_err(|error| error.to_string())?;
    let parent = source.parent().ok_or("source has no parent")?;
    let registry = crate::skills::SkillRegistry::discover_portable(
        vec![crate::skills::SkillDiscoveryRoot::new(
            crate::skills::SkillSourceScope::Managed,
            parent,
            "preview",
        )],
        &[],
    );
    let instance = registry
        .instances()
        .find(|instance| {
            instance.metadata.name
                == source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
        })
        .ok_or("source is not one valid portable skill")?;
    println!(
        "Preview: {} digest {} (disabled after publication)",
        instance.qualified_name, instance.content_digest
    );
    if confirm != Some(instance.content_digest.as_str()) {
        println!(
            "No changes made. Re-run with --confirm {}",
            instance.content_digest
        );
        return Ok(());
    }
    std::fs::create_dir_all(destination_root).map_err(|error| error.to_string())?;
    let version_root = destination_root.join(&instance.content_digest[..12]);
    let destination = version_root.join(&instance.metadata.name);
    if destination.exists() {
        return Err("installation destination already exists".into());
    }
    let staging = destination_root.join(format!(".skill-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let copy_result = (|| {
        for file in &instance.files {
            let from = source.join(&file.relative_path);
            let to = staging.join(&file.relative_path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            std::fs::copy(from, to).map_err(|error| error.to_string())?;
        }
        std::fs::create_dir(&version_root).map_err(|error| error.to_string())?;
        std::fs::rename(&staging, &destination).map_err(|error| error.to_string())
    })();
    if copy_result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_dir(&version_root);
    }
    copy_result?;
    let published = crate::skills::SkillRegistry::discover_portable(
        vec![crate::skills::SkillDiscoveryRoot::new(
            crate::skills::SkillSourceScope::Managed,
            destination_root,
            "managed",
        )],
        &[],
    );
    let published = published
        .instances()
        .find(|candidate| candidate.content_digest == instance.content_digest)
        .ok_or("published skill failed exact verification")?;
    let mut state = read_state(state_path)?;
    state.records.insert(
        published.instance_id.clone(),
        ExactRecord {
            digest: published.content_digest.clone(),
            enabled: false,
        },
    );
    atomic_private_json(state_path, &state)?;
    println!("Installed disabled skill at {}", destination.display());
    Ok(())
}

fn new_skill(name: &str, destination: &Path, confirm: Option<&str>) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || name.contains("--")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err("invalid portable skill name".into());
    }
    let body = format!("---\nname: {name}\ndescription: Describe when to use this skill\n---\n\nAdd instructions here.\n");
    let digest = format!("{:x}", sha2::Sha256::digest(body.as_bytes()));
    println!(
        "Preview: create {}/SKILL.md token {}",
        destination.display(),
        digest
    );
    if confirm != Some(digest.as_str()) {
        println!("No changes made. Re-run with --confirm {digest}");
        return Ok(());
    }
    let target = destination.join(name);
    if target.exists() {
        return Err("destination already exists".into());
    }
    std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::write(target.join("SKILL.md"), body) {
        let _ = std::fs::remove_dir(&target);
        return Err(error.to_string());
    }
    println!("Created {}", target.display());
    Ok(())
}
