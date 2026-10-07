//! Digest-pinned, one-prompt skill bundles.
//!
//! Bundle files are inert YAML metadata. Expansion produces exact skill
//! selectors in the ordinary prompt path and grants no additional authority.

use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::skills::SkillRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_BUNDLE_BYTES: u64 = 64 * 1024;
const MAX_BUNDLES: usize = 128;
const MAX_MEMBERS: usize = 5;
const MAX_INSTRUCTION_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BundleScope {
    Workspace,
    Profile,
    Foreign,
}

impl BundleScope {
    fn writable(self) -> bool {
        !matches!(self, Self::Foreign)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BundleMember {
    pub selector: String,
    pub instance_id: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillBundle {
    pub name: String,
    pub description: String,
    pub instruction: Option<String>,
    pub members: Vec<BundleMember>,
    pub scope: BundleScope,
    pub path: PathBuf,
    pub digest: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SkillBundleRegistry {
    bundles: BTreeMap<String, SkillBundle>,
    diagnostics: Vec<String>,
    recovered_journal_ids: Vec<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    name: String,
    description: String,
    skills: Vec<String>,
    #[serde(default)]
    instruction: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BundleJournal {
    version: u32,
    id: uuid::Uuid,
    state: String,
    action: String,
    target: PathBuf,
    backup: Option<PathBuf>,
    before_digest: Option<String>,
    after_digest: Option<String>,
}

impl SkillBundleRegistry {
    pub(crate) fn discover(
        paths: &RuntimePaths,
        profile: &ActiveProfile,
        workspace: &Path,
        skills: &SkillRegistry,
        occupied_commands: &BTreeSet<String>,
    ) -> Self {
        let roots = [
            (BundleScope::Workspace, workspace.join(".ca/skill-bundles")),
            (BundleScope::Profile, paths.skill_bundles_dir(profile)),
            (
                BundleScope::Foreign,
                paths.home.join(".hermes/skill-bundles"),
            ),
            (
                BundleScope::Foreign,
                paths.home.join(".config/hermes/skill-bundles"),
            ),
        ];
        let mut registry = Self::default();
        let skill_names = skills
            .instances()
            .map(|instance| instance.metadata.name.clone())
            .collect::<BTreeSet<_>>();
        for (scope, root) in roots {
            if scope.writable() {
                recover_journals(
                    &root,
                    &mut registry.diagnostics,
                    &mut registry.recovered_journal_ids,
                );
            }
            let mut files = bounded_yaml_files(&root, &mut registry.diagnostics);
            files.sort();
            for path in files {
                if registry.bundles.len() >= MAX_BUNDLES {
                    registry
                        .diagnostics
                        .push("skill bundle catalog reached its 128-entry bound".into());
                    break;
                }
                match parse_bundle(&path, scope, skills) {
                    Ok(bundle) => {
                        if crate::tui::commands::find_command(&bundle.name).is_some()
                            || occupied_commands.contains(&bundle.name)
                            || skill_names.contains(&bundle.name)
                        {
                            registry.diagnostics.push(format!(
                                "{}: bundle name {} collides with an existing command or skill",
                                path.display(),
                                bundle.name
                            ));
                            continue;
                        }
                        if registry.bundles.contains_key(&bundle.name) {
                            registry.diagnostics.push(format!(
                                "{}: duplicate bundle {} is ignored",
                                path.display(),
                                bundle.name
                            ));
                            continue;
                        }
                        registry.bundles.insert(bundle.name.clone(), bundle);
                    }
                    Err(error) => registry
                        .diagnostics
                        .push(format!("{}: {error}", path.display())),
                }
            }
        }
        registry
    }

    pub(crate) fn bundles(&self) -> impl Iterator<Item = &SkillBundle> {
        self.bundles.values()
    }

    pub(crate) fn find(&self, name: &str) -> Option<&SkillBundle> {
        self.bundles.get(&name.trim().to_ascii_lowercase())
    }

    pub(crate) fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub(crate) fn recovered_journal_ids(&self) -> &[uuid::Uuid] {
        &self.recovered_journal_ids
    }
}

fn parse_bundle(
    path: &Path,
    scope: BundleScope,
    skills: &SkillRegistry,
) -> Result<SkillBundle, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BUNDLE_BYTES
    {
        return Err("bundle must be a bounded regular YAML file".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let parsed: BundleFile = serde_yaml_ng::from_slice(&bytes)
        .map_err(|error| format!("invalid bundle YAML: {error}"))?;
    validate_name(&parsed.name)?;
    if parsed.description.trim().is_empty() || parsed.description.chars().count() > 512 {
        return Err("bundle description must contain 1 to 512 characters".into());
    }
    if !(2..=MAX_MEMBERS).contains(&parsed.skills.len()) {
        return Err("bundle must contain two to five skills".into());
    }
    if parsed
        .instruction
        .as_ref()
        .is_some_and(|instruction| instruction.len() > MAX_INSTRUCTION_BYTES)
    {
        return Err("bundle instruction exceeds its byte bound".into());
    }
    let mut seen = BTreeSet::new();
    let mut members = Vec::new();
    let mut failures = Vec::new();
    for selector in &parsed.skills {
        match skills.resolve_instance(selector, true) {
            Ok(instance) if seen.insert(instance.instance_id.clone()) => {
                members.push(BundleMember {
                    selector: selector.clone(),
                    instance_id: instance.instance_id.clone(),
                    digest: instance.content_digest.clone(),
                })
            }
            Ok(_) => failures.push(format!("duplicate member {selector}")),
            Err(error) => failures.push(format!("{selector}: {error}")),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "bundle could not resolve atomically: {}",
            failures.join("; ")
        ));
    }
    Ok(SkillBundle {
        name: parsed.name,
        description: parsed.description,
        instruction: parsed.instruction,
        members,
        scope,
        path: path.to_path_buf(),
        digest: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err(format!("invalid bundle name: {name}"));
    }
    Ok(())
}

fn bounded_yaml_files(root: &Path, diagnostics: &mut Vec<String>) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    if fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        diagnostics.push(format!("{}: symlinked bundle root ignored", root.display()));
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(root) else {
        diagnostics.push(format!("{}: bundle root could not be read", root.display()));
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yaml" | "yml")
            )
        })
        .take(MAX_BUNDLES)
        .collect()
}

pub(crate) fn create_bundle(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    workspace: &Path,
    workspace_scope: bool,
    name: &str,
    selectors: &[String],
    skills: &SkillRegistry,
) -> Result<uuid::Uuid, String> {
    validate_name(name)?;
    if !(2..=MAX_MEMBERS).contains(&selectors.len()) {
        return Err("bundle creation requires two to five exact skill selectors".into());
    }
    if skills
        .instances()
        .any(|instance| instance.metadata.name == name)
    {
        return Err(format!(
            "bundle name collides with an existing skill: {name}"
        ));
    }
    let mut pinned = Vec::new();
    let mut failures = Vec::new();
    for selector in selectors {
        match skills.resolve_instance(selector, true) {
            Ok(instance) => pinned.push(instance.qualified_name.clone()),
            Err(error) => failures.push(format!("{selector}: {error}")),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "bundle creation resolved no partial stack: {}",
            failures.join("; ")
        ));
    }
    let root = if workspace_scope {
        workspace.join(".ca/skill-bundles")
    } else {
        paths.skill_bundles_dir(profile)
    };
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    validate_bundle_directory(&root)?;
    let target = root.join(format!("{name}.yaml"));
    if target.exists() {
        return Err(format!("bundle already exists: {name}"));
    }
    let file = BundleFile {
        name: name.into(),
        description: format!("Activate {} together for one prompt", pinned.join(", ")),
        skills: pinned,
        instruction: None,
    };
    let bytes = serde_yaml_ng::to_string(&file)
        .map_err(|error| error.to_string())?
        .into_bytes();
    mutate_bundle_file(&root, &target, "create", None, Some(&bytes))
}

pub(crate) fn delete_bundle(bundle: &SkillBundle) -> Result<uuid::Uuid, String> {
    if !bundle.scope.writable() {
        return Err("foreign bundles are read-only".into());
    }
    let root = bundle
        .path
        .parent()
        .ok_or_else(|| "bundle target has no parent".to_string())?;
    let current = fs::read(&bundle.path).map_err(|error| error.to_string())?;
    if format!("{:x}", Sha256::digest(&current)) != bundle.digest {
        return Err("bundle changed since discovery".into());
    }
    mutate_bundle_file(root, &bundle.path, "delete", Some(&current), None)
}

fn mutate_bundle_file(
    root: &Path,
    target: &Path,
    action: &str,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<uuid::Uuid, String> {
    let id = uuid::Uuid::new_v4();
    validate_bundle_directory(root)?;
    let journal_dir = root.join(".journal");
    fs::create_dir_all(&journal_dir).map_err(|error| error.to_string())?;
    validate_bundle_directory(&journal_dir)?;
    let backup = before.map(|_| journal_dir.join(format!("{id}.backup")));
    if let (Some(bytes), Some(path)) = (before, backup.as_ref()) {
        write_owner_file(path, bytes, true)?;
    }
    let journal_path = journal_dir.join(format!("{id}.json"));
    let mut journal = BundleJournal {
        version: 1,
        id,
        state: "prepared".into(),
        action: action.into(),
        target: target.to_path_buf(),
        backup: backup.clone(),
        before_digest: before.map(|bytes| format!("{:x}", Sha256::digest(bytes))),
        after_digest: after.map(|bytes| format!("{:x}", Sha256::digest(bytes))),
    };
    write_journal(&journal_path, &journal)?;
    match after {
        Some(bytes) => write_owner_file(target, bytes, true)?,
        None => fs::remove_file(target).map_err(|error| error.to_string())?,
    }
    journal.state = "committed".into();
    write_journal(&journal_path, &journal)?;
    Ok(id)
}

fn write_owner_file(path: &Path, bytes: &[u8], replace: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "bundle file has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    validate_bundle_directory(parent)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !replace {
            return Err("bundle file already exists".into());
        }
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("bundle file target is not a regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err("bundle file target has multiple hard links".into());
            }
        }
    }
    let temporary = parent.join(format!(".bundle-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), String> {
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn validate_bundle_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("bundle directory must be a real directory".into());
    }
    Ok(())
}

fn write_journal(path: &Path, journal: &BundleJournal) -> Result<(), String> {
    write_owner_file(
        path,
        &serde_json::to_vec_pretty(journal).map_err(|error| error.to_string())?,
        true,
    )
}

fn recover_journals(root: &Path, diagnostics: &mut Vec<String>, recovered: &mut Vec<uuid::Uuid>) {
    let journal_dir = root.join(".journal");
    let Ok(entries) = fs::read_dir(&journal_dir) else {
        return;
    };
    for path in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
    {
        let result = (|| -> Result<(), String> {
            let mut journal: BundleJournal =
                serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            if journal.version != 1 || journal.state != "prepared" {
                return Ok(());
            }
            if let Some(backup) = journal.backup.as_ref() {
                let bytes = fs::read(backup).map_err(|error| error.to_string())?;
                write_owner_file(&journal.target, &bytes, true)?;
            } else if journal.target.exists() {
                fs::remove_file(&journal.target).map_err(|error| error.to_string())?;
            }
            journal.state = "recovered".into();
            write_journal(&path, &journal)?;
            recovered.push(journal.id);
            Ok(())
        })();
        if let Err(error) = result {
            diagnostics.push(format!(
                "{}: bundle journal recovery failed: {error}",
                path.display()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{SkillDiscoveryRoot, SkillLoadSet, SkillSourceScope};

    fn write_skill(root: &Path, name: &str) {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name}\n---\nUse {name}.\n"),
        )
        .unwrap();
    }

    #[test]
    fn bundle_resolution_is_atomic_digest_pinned_and_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills");
        write_skill(&skill_root, "alpha");
        write_skill(&skill_root, "beta");
        let skills = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &skill_root,
                "test",
            )],
            &[],
        );
        let bundle_root = temp.path().join("workspace/.ca/skill-bundles");
        fs::create_dir_all(&bundle_root).unwrap();
        fs::write(
            bundle_root.join("pair.yaml"),
            "name: pair\ndescription: Pair\nskills: [alpha, beta]\n",
        )
        .unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        let registry = SkillBundleRegistry::discover(
            &paths,
            &ActiveProfile::Default,
            &temp.path().join("workspace"),
            &skills,
            &BTreeSet::new(),
        );
        let pair = registry.find("pair").unwrap();
        assert_eq!(pair.members.len(), 2);
        assert!(pair.members.iter().all(|member| member.digest.len() == 64));

        fs::write(
            bundle_root.join("broken.yaml"),
            "name: broken\ndescription: Broken\nskills: [alpha, missing]\n",
        )
        .unwrap();
        let registry = SkillBundleRegistry::discover(
            &paths,
            &ActiveProfile::Default,
            &temp.path().join("workspace"),
            &skills,
            &BTreeSet::new(),
        );
        assert!(registry.find("broken").is_none());
        assert!(registry
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.contains("missing")));
    }

    #[test]
    fn bundle_creation_rejects_singletons_limits_and_skill_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills");
        write_skill(&skill_root, "alpha");
        write_skill(&skill_root, "beta");
        let skills = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &skill_root,
                "test",
            )],
            &[],
        );
        let paths = RuntimePaths::from_home(temp.path());
        let workspace = temp.path().join("workspace");
        assert!(create_bundle(
            &paths,
            &ActiveProfile::Default,
            &workspace,
            true,
            "single",
            &["alpha".into()],
            &skills,
        )
        .unwrap_err()
        .contains("two to five"));
        assert!(create_bundle(
            &paths,
            &ActiveProfile::Default,
            &workspace,
            true,
            "alpha",
            &["alpha".into(), "beta".into()],
            &skills,
        )
        .unwrap_err()
        .contains("existing skill"));
        assert!(create_bundle(
            &paths,
            &ActiveProfile::Default,
            &workspace,
            true,
            "too-many",
            &[
                "alpha".into(),
                "beta".into(),
                "alpha".into(),
                "beta".into(),
                "alpha".into(),
                "beta".into(),
            ],
            &skills,
        )
        .unwrap_err()
        .contains("two to five"));
    }

    #[test]
    fn discovery_rejects_builtin_and_ambiguous_collisions_without_partial_bundles() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        write_skill(&first, "alpha");
        write_skill(&second, "alpha");
        write_skill(&first, "beta");
        let skills = SkillRegistry::discover_portable(
            vec![
                SkillDiscoveryRoot::new(SkillSourceScope::Workspace, &first, "first"),
                SkillDiscoveryRoot::new(SkillSourceScope::User, &second, "second"),
            ],
            &[],
        );
        let workspace = temp.path().join("workspace");
        let root = workspace.join(".ca/skill-bundles");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("ambiguous.yaml"),
            "name: ambiguous\ndescription: Ambiguous\nskills: [alpha, beta]\n",
        )
        .unwrap();
        let exact_alpha = skills
            .instances()
            .find(|instance| {
                instance.metadata.name == "alpha"
                    && instance.source_scope == SkillSourceScope::Workspace
            })
            .unwrap()
            .qualified_name
            .clone();
        fs::write(
            root.join("learn.yaml"),
            format!("name: learn\ndescription: Collision\nskills: [{exact_alpha}, beta]\n"),
        )
        .unwrap();
        let registry = SkillBundleRegistry::discover(
            &RuntimePaths::from_home(temp.path()),
            &ActiveProfile::Default,
            &workspace,
            &skills,
            &BTreeSet::new(),
        );
        assert!(registry.find("ambiguous").is_none());
        assert!(registry.find("learn").is_none());
        assert!(registry
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.contains("ambiguous")));
        assert!(registry
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.contains("collides")));
    }

    #[test]
    fn pinned_bundle_member_detects_digest_drift_before_activation() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills");
        write_skill(&skill_root, "alpha");
        write_skill(&skill_root, "beta");
        let skills = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &skill_root,
                "test",
            )],
            &[],
        );
        let workspace = temp.path().join("workspace");
        let root = workspace.join(".ca/skill-bundles");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("pair.yaml"),
            "name: pair\ndescription: Pair\nskills: [alpha, beta]\n",
        )
        .unwrap();
        let registry = SkillBundleRegistry::discover(
            &RuntimePaths::from_home(temp.path()),
            &ActiveProfile::Default,
            &workspace,
            &skills,
            &BTreeSet::new(),
        );
        let member = registry.find("pair").unwrap().members[0].clone();
        fs::write(
            skill_root.join("alpha/SKILL.md"),
            "---\nname: alpha\ndescription: changed\n---\nChanged.\n",
        )
        .unwrap();
        let error = skills
            .load_for_task(&member.instance_id, true, &mut SkillLoadSet::default())
            .unwrap_err();
        assert!(error.to_string().contains("changed"));
    }

    #[test]
    fn foreign_bundles_are_read_only_and_prepared_journals_recover() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills");
        write_skill(&skill_root, "alpha");
        write_skill(&skill_root, "beta");
        let skills = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &skill_root,
                "test",
            )],
            &[],
        );
        let paths = RuntimePaths::from_home(temp.path());
        let foreign_root = temp.path().join(".hermes/skill-bundles");
        fs::create_dir_all(&foreign_root).unwrap();
        fs::write(
            foreign_root.join("foreign.yaml"),
            "name: foreign\ndescription: Foreign\nskills: [alpha, beta]\n",
        )
        .unwrap();

        let workspace = temp.path().join("workspace");
        let owner_root = workspace.join(".ca/skill-bundles");
        let journal_root = owner_root.join(".journal");
        fs::create_dir_all(&journal_root).unwrap();
        let target = owner_root.join("interrupted.yaml");
        fs::write(&target, b"partial").unwrap();
        let journal_id = uuid::Uuid::new_v4();
        write_journal(
            &journal_root.join(format!("{journal_id}.json")),
            &BundleJournal {
                version: 1,
                id: journal_id,
                state: "prepared".into(),
                action: "create".into(),
                target: target.clone(),
                backup: None,
                before_digest: None,
                after_digest: Some(format!("{:x}", Sha256::digest(b"partial"))),
            },
        )
        .unwrap();

        let registry = SkillBundleRegistry::discover(
            &paths,
            &ActiveProfile::Default,
            &workspace,
            &skills,
            &BTreeSet::new(),
        );
        assert!(!target.exists());
        assert_eq!(registry.recovered_journal_ids(), [journal_id]);
        let foreign = registry.find("foreign").unwrap();
        assert!(delete_bundle(foreign).unwrap_err().contains("read-only"));
    }

    #[test]
    fn combined_bundle_members_use_the_existing_task_skill_budget() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills");
        for name in ["alpha", "beta"] {
            let root = skill_root.join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: {name}\n---\n{}\n",
                    "bounded procedure ".repeat(1_800)
                ),
            )
            .unwrap();
        }
        let skills = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &skill_root,
                "test",
            )],
            &[],
        );
        let mut loads = SkillLoadSet::default();
        skills.load_for_task("alpha", true, &mut loads).unwrap();
        skills.load_for_task("beta", true, &mut loads).unwrap();
        assert!(skills
            .task_prompt(&loads)
            .unwrap_err()
            .to_string()
            .contains("budget"));
    }
}
