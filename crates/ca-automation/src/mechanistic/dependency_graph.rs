//! Bounded, deterministic package ownership and reverse-dependency planning.
//!
//! Graph construction reads only tracked manifests. Rust and Swift structured
//! metadata commands run through the existing isolated no-shell supervisor;
//! Node and Python manifests are parsed directly because their workspace
//! relationships are already declarative.

use super::MechanisticCacheStatusV1;
use crate::cancellation::CancellationToken;
use crate::paths::Paths;
use crate::verification::isolation::IsolationSession;
use crate::verification::recipe::{
    CachePolicy, CommandKind, FailurePolicy, Recipe, RecipeSpec, VerificationCommand,
};
use crate::verification::registry::{
    is_deterministic_command_structured_allowed, recipe_spec_to_command, validate_recipe_spec,
};
use crate::verification::runner::{
    run_isolated_structured_command_in_session_with_cancellation, DeterministicResultCategory,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

const MAX_MANIFESTS: usize = 128;
const MAX_PACKAGES: usize = 512;
const MAX_SELECTED_PACKAGES: usize = 16;
const MAX_SELECTED_PATHS: usize = 64;
const MAX_METADATA_BYTES: usize = 2 * 1024 * 1024;
const MAX_GRAPH_CACHE_ENTRIES: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum Ecosystem {
    Rust,
    Node,
    Python,
    Swift,
}

impl Ecosystem {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Node => "node",
            Self::Python => "python",
            Self::Swift => "swift",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PackageNode {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) ecosystem: Ecosystem,
    pub(crate) root: String,
    pub(crate) dependencies: BTreeSet<String>,
    pub(crate) targets: Vec<String>,
    pub(crate) test_paths: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct VerificationGraph {
    packages: BTreeMap<String, PackageNode>,
}

#[derive(Debug, Clone)]
pub(crate) struct DependencySelection {
    pub(crate) packages: Vec<PackageNode>,
    pub(crate) owning_packages: Vec<String>,
    pub(crate) reverse_dependencies: Vec<String>,
    pub(crate) selected_targets: Vec<String>,
    pub(crate) ecosystems: Vec<String>,
    pub(crate) relevant_test_paths: Vec<String>,
    pub(crate) cache_status: MechanisticCacheStatusV1,
    pub(crate) reason_codes: Vec<String>,
}

static GRAPH_CACHE: OnceLock<Mutex<VecDeque<(String, VerificationGraph)>>> = OnceLock::new();

fn graph_cache() -> &'static Mutex<VecDeque<(String, VerificationGraph)>> {
    GRAPH_CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(crate) fn select_dependencies(
    paths: &Paths,
    tracked: &BTreeSet<String>,
    changed_paths: &[String],
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> Result<DependencySelection> {
    if changed_paths.is_empty() || changed_paths.len() > MAX_SELECTED_PATHS {
        bail!("dependency planning requires 1-{MAX_SELECTED_PATHS} changed paths");
    }
    let key = graph_cache_key(&paths.root, tracked)?;
    let (graph, cache_status) = {
        let cache = graph_cache()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some((_, graph)) = cache.iter().find(|(candidate, _)| candidate == &key) {
            (graph.clone(), MechanisticCacheStatusV1::Hit)
        } else {
            drop(cache);
            let graph = build_graph(paths, tracked, isolation, cancellation)?;
            let mut cache = graph_cache()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            cache.push_front((key, graph.clone()));
            cache.truncate(MAX_GRAPH_CACHE_ENTRIES);
            (graph, MechanisticCacheStatusV1::Miss)
        }
    };
    select_from_graph(&graph, changed_paths, cache_status)
}

fn build_graph(
    paths: &Paths,
    tracked: &BTreeSet<String>,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
) -> Result<VerificationGraph> {
    let manifests = tracked
        .iter()
        .filter(|path| is_graph_input(path))
        .cloned()
        .collect::<Vec<_>>();
    if manifests.len() > MAX_MANIFESTS {
        bail!("dependency graph exceeds the {MAX_MANIFESTS}-manifest limit");
    }
    let mut graph = VerificationGraph::default();
    build_rust_graph(paths, tracked, isolation, cancellation, &mut graph)?;
    build_node_graph(&paths.root, tracked, &mut graph)?;
    build_python_graph(&paths.root, tracked, &mut graph)?;
    build_swift_graph(paths, tracked, isolation, cancellation, &mut graph)?;
    if graph.packages.is_empty() {
        bail!("no supported package graph is available");
    }
    if graph.packages.len() > MAX_PACKAGES {
        bail!("dependency graph exceeds the {MAX_PACKAGES}-package limit");
    }
    Ok(graph)
}

fn select_from_graph(
    graph: &VerificationGraph,
    changed_paths: &[String],
    cache_status: MechanisticCacheStatusV1,
) -> Result<DependencySelection> {
    let mut owner_ids = BTreeSet::new();
    for path in changed_paths {
        let expected_ecosystem = ecosystem_for_path(path);
        let owner = graph
            .packages
            .values()
            .filter(|package| {
                expected_ecosystem.is_none_or(|ecosystem| package.ecosystem == ecosystem)
            })
            .filter(|package| path_in_root(path, &package.root))
            .max_by_key(|package| package.root.len())
            .with_context(|| format!("no package owns changed path `{path}`"))?;
        owner_ids.insert(owner.id.clone());
    }
    let ecosystems = owner_ids
        .iter()
        .filter_map(|id| graph.packages.get(id).map(|package| package.ecosystem))
        .collect::<BTreeSet<_>>();
    if ecosystems.len() != 1 {
        bail!("changed paths span multiple ecosystems");
    }

    let mut selected = owner_ids.clone();
    loop {
        let mut changed = false;
        for package in graph.packages.values() {
            if selected.contains(&package.id) {
                continue;
            }
            if package
                .dependencies
                .iter()
                .any(|dependency| selected.contains(dependency))
            {
                selected.insert(package.id.clone());
                changed = true;
                if selected.len() > MAX_SELECTED_PACKAGES {
                    bail!(
                        "reverse dependency closure exceeds the {MAX_SELECTED_PACKAGES}-package limit"
                    );
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut packages = selected
        .iter()
        .filter_map(|id| graph.packages.get(id).cloned())
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    let owning_packages = owner_ids
        .iter()
        .filter_map(|id| graph.packages.get(id).map(|package| package.name.clone()))
        .collect::<Vec<_>>();
    let reverse_dependencies = selected
        .difference(&owner_ids)
        .filter_map(|id| graph.packages.get(id).map(|package| package.name.clone()))
        .collect::<Vec<_>>();
    let selected_targets = packages
        .iter()
        .flat_map(|package| {
            package
                .targets
                .iter()
                .map(move |target| format!("{}:{target}", package.name))
        })
        .take(MAX_SELECTED_PATHS)
        .collect::<Vec<_>>();
    let relevant_test_paths = relevant_tests(&packages, changed_paths);
    Ok(DependencySelection {
        packages,
        owning_packages,
        reverse_dependencies,
        selected_targets,
        ecosystems: ecosystems
            .into_iter()
            .map(|ecosystem| ecosystem.as_str().to_string())
            .collect(),
        relevant_test_paths,
        cache_status,
        reason_codes: vec!["dependency_graph_transitive_reverse_closure".to_string()],
    })
}

fn relevant_tests(packages: &[PackageNode], changed_paths: &[String]) -> Vec<String> {
    let stems = changed_paths
        .iter()
        .filter_map(|path| Path::new(path).file_stem().and_then(|value| value.to_str()))
        .map(|value| value.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut ranked = packages
        .iter()
        .flat_map(|package| package.test_paths.iter())
        .map(|path| {
            let folded = path.to_ascii_lowercase();
            let score = stems.iter().filter(|stem| folded.contains(*stem)).count();
            (score, path.clone())
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    ranked
        .into_iter()
        .filter(|(score, _)| *score > 0)
        .map(|(_, path)| path)
        .take(MAX_SELECTED_PATHS)
        .collect()
}

fn build_rust_graph(
    paths: &Paths,
    tracked: &BTreeSet<String>,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
    graph: &mut VerificationGraph,
) -> Result<()> {
    let manifests = root_manifests(tracked, "Cargo.toml");
    for manifest in manifests {
        let cwd = parent_string(&manifest);
        let command = graph_command(Recipe::CargoMetadata, cwd.clone(), "graph-cargo-metadata")?;
        let result = run_isolated_structured_command_in_session_with_cancellation(
            paths,
            &command,
            isolation,
            cancellation,
        )?;
        if result.category != DeterministicResultCategory::Passed {
            bail!("isolated cargo metadata failed for `{manifest}`");
        }
        if result.stdout_tail.len() > MAX_METADATA_BYTES {
            bail!("cargo metadata exceeds the {MAX_METADATA_BYTES}-byte limit");
        }
        parse_cargo_metadata(&result.stdout_tail, graph)?;
    }
    Ok(())
}

fn parse_cargo_metadata(text: &str, graph: &mut VerificationGraph) -> Result<()> {
    let value: Value = serde_json::from_str(text).context("parse bounded cargo metadata")?;
    let workspace_root = value
        .get("workspace_root")
        .and_then(Value::as_str)
        .context("cargo metadata omitted workspace_root")?;
    let workspace_members = value
        .get("workspace_members")
        .and_then(Value::as_array)
        .context("cargo metadata omitted workspace_members")?
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .context("cargo metadata omitted packages")?;
    let mut name_to_id = BTreeMap::new();
    for package in packages {
        let id = package
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if workspace_members.contains(id) && !id.is_empty() && !name.is_empty() {
            name_to_id.insert(name.to_string(), format!("rust:{id}"));
        }
    }
    for package in packages {
        let raw_id = package
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !workspace_members.contains(raw_id) {
            continue;
        }
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .context("cargo metadata package omitted name")?
            .to_string();
        let manifest = package
            .get("manifest_path")
            .and_then(Value::as_str)
            .context("cargo metadata package omitted manifest_path")?;
        let relative_manifest = metadata_relative(workspace_root, manifest)
            .context("cargo metadata manifest escaped its workspace")?;
        let root = parent_string(&relative_manifest).unwrap_or_default();
        let dependencies = package
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|dependency| dependency.get("name").and_then(Value::as_str))
            .filter_map(|dependency| name_to_id.get(dependency).cloned())
            .collect::<BTreeSet<_>>();
        let mut targets = Vec::new();
        let mut test_paths = Vec::new();
        for target in package
            .get("targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let target_name = target
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let kinds = target
                .get("kind")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            targets.push(target_name);
            if kinds.contains(&"test") {
                if let Some(src) = target.get("src_path").and_then(Value::as_str) {
                    if let Some(relative) = metadata_relative(workspace_root, src) {
                        test_paths.push(relative);
                    }
                }
            }
        }
        let id = format!("rust:{raw_id}");
        graph.packages.insert(
            id.clone(),
            PackageNode {
                id,
                name,
                ecosystem: Ecosystem::Rust,
                root,
                dependencies,
                targets,
                test_paths,
            },
        );
    }
    Ok(())
}

fn build_node_graph(
    repo: &Path,
    tracked: &BTreeSet<String>,
    graph: &mut VerificationGraph,
) -> Result<()> {
    let manifests = tracked
        .iter()
        .filter(|path| Path::new(path).file_name().and_then(|v| v.to_str()) == Some("package.json"))
        .take(MAX_MANIFESTS + 1)
        .cloned()
        .collect::<Vec<_>>();
    if manifests.len() > MAX_MANIFESTS {
        bail!("Node graph exceeds the manifest limit");
    }
    let mut parsed = Vec::new();
    for manifest in manifests {
        let bytes = read_tracked_manifest(repo, &manifest)?;
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse tracked Node manifest `{manifest}`"))?;
        let Some(name) = value.get("name").and_then(Value::as_str) else {
            continue;
        };
        let dependencies = ["dependencies", "devDependencies", "peerDependencies"]
            .into_iter()
            .filter_map(|field| value.get(field).and_then(Value::as_object))
            .flat_map(|items| items.keys().cloned())
            .collect::<BTreeSet<_>>();
        parsed.push((manifest, name.to_string(), dependencies));
    }
    let names = parsed
        .iter()
        .map(|(_, name, _)| (name.clone(), format!("node:{name}")))
        .collect::<BTreeMap<_, _>>();
    for (manifest, name, dependencies) in parsed {
        let id = format!("node:{name}");
        let root = parent_string(&manifest).unwrap_or_default();
        graph.packages.insert(
            id.clone(),
            PackageNode {
                id,
                name,
                ecosystem: Ecosystem::Node,
                root: root.clone(),
                dependencies: dependencies
                    .into_iter()
                    .filter_map(|dependency| names.get(&dependency).cloned())
                    .collect(),
                targets: vec!["test".to_string()],
                test_paths: tracked_tests_under(tracked, &root, Ecosystem::Node),
            },
        );
    }
    Ok(())
}

fn build_python_graph(
    repo: &Path,
    tracked: &BTreeSet<String>,
    graph: &mut VerificationGraph,
) -> Result<()> {
    let manifests = tracked
        .iter()
        .filter(|path| {
            Path::new(path).file_name().and_then(|v| v.to_str()) == Some("pyproject.toml")
        })
        .take(MAX_MANIFESTS + 1)
        .cloned()
        .collect::<Vec<_>>();
    if manifests.len() > MAX_MANIFESTS {
        bail!("Python graph exceeds the manifest limit");
    }
    let mut parsed = Vec::new();
    for manifest in manifests {
        let bytes = read_tracked_manifest(repo, &manifest)?;
        let value: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)
            .with_context(|| format!("parse tracked Python manifest `{manifest}`"))?;
        let name = value
            .get("project")
            .and_then(|project| project.get("name"))
            .and_then(toml::Value::as_str)
            .or_else(|| {
                value
                    .get("tool")
                    .and_then(|tool| tool.get("poetry"))
                    .and_then(|poetry| poetry.get("name"))
                    .and_then(toml::Value::as_str)
            });
        let Some(name) = name else { continue };
        let mut dependencies = BTreeSet::new();
        if let Some(items) = value
            .get("project")
            .and_then(|project| project.get("dependencies"))
            .and_then(toml::Value::as_array)
        {
            for item in items.iter().filter_map(toml::Value::as_str) {
                if let Some(name) = python_requirement_name(item) {
                    dependencies.insert(name);
                }
            }
        }
        if let Some(items) = value
            .get("tool")
            .and_then(|tool| tool.get("poetry"))
            .and_then(|poetry| poetry.get("dependencies"))
            .and_then(toml::Value::as_table)
        {
            dependencies.extend(items.keys().cloned());
        }
        parsed.push((manifest, name.to_string(), dependencies));
    }
    let names = parsed
        .iter()
        .map(|(_, name, _)| (normalize_python_name(name), format!("python:{name}")))
        .collect::<BTreeMap<_, _>>();
    for (manifest, name, dependencies) in parsed {
        let id = format!("python:{name}");
        let root = parent_string(&manifest).unwrap_or_default();
        graph.packages.insert(
            id.clone(),
            PackageNode {
                id,
                name,
                ecosystem: Ecosystem::Python,
                root: root.clone(),
                dependencies: dependencies
                    .into_iter()
                    .filter_map(|dependency| {
                        names.get(&normalize_python_name(&dependency)).cloned()
                    })
                    .collect(),
                targets: vec!["pytest".to_string()],
                test_paths: tracked_tests_under(tracked, &root, Ecosystem::Python),
            },
        );
    }
    Ok(())
}

fn build_swift_graph(
    paths: &Paths,
    tracked: &BTreeSet<String>,
    isolation: &IsolationSession,
    cancellation: &CancellationToken,
    graph: &mut VerificationGraph,
) -> Result<()> {
    for manifest in root_manifests(tracked, "Package.swift") {
        let cwd = parent_string(&manifest);
        let command = graph_command(Recipe::SwiftDumpPackage, cwd.clone(), "graph-swift-package")?;
        let result = run_isolated_structured_command_in_session_with_cancellation(
            paths,
            &command,
            isolation,
            cancellation,
        )?;
        if result.category != DeterministicResultCategory::Passed {
            bail!("isolated Swift package graph failed for `{manifest}`");
        }
        if result.stdout_tail.len() > MAX_METADATA_BYTES {
            bail!("Swift package metadata exceeds the bounded limit");
        }
        let value: Value = serde_json::from_str(&result.stdout_tail)
            .with_context(|| format!("parse Swift package metadata for `{manifest}`"))?;
        parse_swift_metadata(&value, cwd.unwrap_or_default(), tracked, graph)?;
    }
    Ok(())
}

fn parse_swift_metadata(
    value: &Value,
    root: String,
    tracked: &BTreeSet<String>,
    graph: &mut VerificationGraph,
) -> Result<()> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .context("Swift package metadata omitted name")?
        .to_string();
    let targets = value
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let id = format!("swift:{name}");
    graph.packages.insert(
        id.clone(),
        PackageNode {
            id,
            name,
            ecosystem: Ecosystem::Swift,
            root: root.clone(),
            dependencies: BTreeSet::new(),
            targets,
            test_paths: tracked_tests_under(tracked, &root, Ecosystem::Swift),
        },
    );
    Ok(())
}

fn graph_command(recipe: Recipe, cwd: Option<String>, id: &str) -> Result<VerificationCommand> {
    let spec = RecipeSpec {
        id: id.to_string(),
        recipe,
        cwd,
        timeout_seconds: Some(30),
        kind: CommandKind::Unknown,
        failure_policy: FailurePolicy::Stop,
        cache_policy: CachePolicy::Never,
        expected_exit: Some(0),
        source: "mechanistic.dependency_graph.v1".to_string(),
    };
    if !validate_recipe_spec(&spec) {
        bail!("dependency graph recipe is outside the typed registry");
    }
    let command = recipe_spec_to_command(spec);
    if !is_deterministic_command_structured_allowed(&command) {
        bail!("dependency graph recipe is outside the final typed allowlist");
    }
    Ok(command)
}

fn graph_cache_key(repo: &Path, tracked: &BTreeSet<String>) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"codex-team-dependency-graph-v1\0");
    hasher.update(repo.as_os_str().as_encoded_bytes());
    hasher.update(std::env::consts::OS.as_bytes());
    hasher.update(std::env::consts::ARCH.as_bytes());
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    for name in [
        "PATH",
        "RUSTUP_TOOLCHAIN",
        "RUSTC_WRAPPER",
        "CARGO_HOME",
        "NODE_OPTIONS",
        "VIRTUAL_ENV",
        "SWIFT_EXEC",
    ] {
        hasher.update(b"\0env\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        if let Some(value) = std::env::var_os(name) {
            hasher.update(value.to_string_lossy().as_bytes());
        }
    }
    let mut count = 0_usize;
    let mut total = 0_usize;
    for relative in tracked.iter().filter(|path| is_graph_input(path)) {
        count += 1;
        if count > MAX_MANIFESTS {
            bail!("dependency graph exceeds the manifest limit");
        }
        let bytes = read_tracked_manifest(repo, relative)?;
        total = total.saturating_add(bytes.len());
        if total > MAX_METADATA_BYTES {
            bail!("dependency graph inputs exceed the bounded metadata limit");
        }
        hasher.update(relative.as_bytes());
        hasher.update(b"\0");
        hasher.update(Sha256::digest(&bytes));
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn read_tracked_manifest(repo: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = repo.join(relative);
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect tracked manifest `{relative}`"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("dependency manifests must be regular non-symlink files");
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
        bail!("dependency manifests may not be hardlinked");
    }
    if metadata.len() > MAX_METADATA_BYTES as u64 {
        bail!("dependency manifest exceeds the bounded metadata limit");
    }
    fs::read(path).with_context(|| format!("read tracked manifest `{relative}`"))
}

fn is_graph_input(path: &str) -> bool {
    matches!(
        Path::new(path).file_name().and_then(|value| value.to_str()),
        Some(
            "Cargo.toml"
                | "Cargo.lock"
                | "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "pyproject.toml"
                | "Package.swift"
                | "Package.resolved"
        )
    )
}

fn ecosystem_for_path(path: &str) -> Option<Ecosystem> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".rs") {
        Some(Ecosystem::Rust)
    } else if lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
    {
        Some(Ecosystem::Node)
    } else if lower.ends_with(".py") {
        Some(Ecosystem::Python)
    } else if lower.ends_with(".swift") {
        Some(Ecosystem::Swift)
    } else {
        None
    }
}

fn root_manifests(tracked: &BTreeSet<String>, name: &str) -> Vec<String> {
    let manifests = tracked
        .iter()
        .filter(|path| Path::new(path).file_name().and_then(|value| value.to_str()) == Some(name))
        .cloned()
        .collect::<Vec<_>>();
    manifests
        .iter()
        .filter(|manifest| {
            let parent = Path::new(manifest)
                .parent()
                .unwrap_or_else(|| Path::new(""));
            !manifests.iter().any(|candidate| {
                candidate != *manifest
                    && parent_string(candidate)
                        .is_some_and(|root| !root.is_empty() && parent.starts_with(root))
            })
        })
        .cloned()
        .collect()
}

fn metadata_relative(workspace_root: &str, absolute: &str) -> Option<String> {
    let relative = Path::new(absolute).strip_prefix(workspace_root).ok()?;
    let value = relative.to_string_lossy().replace('\\', "/");
    (!value.split('/').any(|part| part == ".." || part.is_empty())).then_some(value)
}

fn parent_string(path: &str) -> Option<String> {
    let parent = Path::new(path).parent()?;
    (!parent.as_os_str().is_empty()).then(|| parent.to_string_lossy().replace('\\', "/"))
}

fn path_in_root(path: &str, root: &str) -> bool {
    root.is_empty() || path == root || path.starts_with(&format!("{root}/"))
}

fn tracked_tests_under(
    tracked: &BTreeSet<String>,
    root: &str,
    ecosystem: Ecosystem,
) -> Vec<String> {
    tracked
        .iter()
        .filter(|path| path_in_root(path, root))
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            match ecosystem {
                Ecosystem::Rust => lower.contains("/tests/") || lower.ends_with("_test.rs"),
                Ecosystem::Node => {
                    lower.contains("/test/")
                        || lower.contains("/tests/")
                        || lower.ends_with(".test.js")
                        || lower.ends_with(".test.ts")
                        || lower.ends_with(".spec.js")
                        || lower.ends_with(".spec.ts")
                }
                Ecosystem::Python => {
                    lower.contains("/tests/")
                        || lower.ends_with("_test.py")
                        || lower
                            .rsplit('/')
                            .next()
                            .is_some_and(|name| name.starts_with("test_"))
                }
                Ecosystem::Swift => lower.contains("/tests/") || lower.ends_with("tests.swift"),
            }
        })
        .take(MAX_SELECTED_PATHS)
        .cloned()
        .collect()
}

fn python_requirement_name(value: &str) -> Option<String> {
    let name = value
        .split(['<', '>', '=', '!', '~', '[', ';', ' '])
        .next()?
        .trim();
    (!name.is_empty()).then(|| normalize_python_name(name))
}

fn normalize_python_name(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if matches!(character, '_' | '.') {
                '-'
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_selection_uses_transitive_reverse_dependencies() {
        let mut graph = VerificationGraph::default();
        for (id, name, root, dependencies) in [
            ("rust:core", "core", "crates/core", vec![]),
            ("rust:ui", "ui", "crates/ui", vec!["rust:core"]),
            ("rust:app", "app", "", vec!["rust:ui"]),
        ] {
            graph.packages.insert(
                id.to_string(),
                PackageNode {
                    id: id.to_string(),
                    name: name.to_string(),
                    ecosystem: Ecosystem::Rust,
                    root: root.to_string(),
                    dependencies: dependencies.into_iter().map(str::to_string).collect(),
                    targets: vec![name.to_string()],
                    test_paths: vec![format!("{root}/tests/{name}_test.rs")],
                },
            );
        }
        let selected = select_from_graph(
            &graph,
            &["crates/core/src/lib.rs".to_string()],
            MechanisticCacheStatusV1::Miss,
        )
        .unwrap();
        assert_eq!(selected.owning_packages, vec!["core"]);
        assert_eq!(selected.reverse_dependencies, vec!["app", "ui"]);
    }

    #[test]
    fn mixed_ecosystems_fail_closed() {
        let mut graph = VerificationGraph::default();
        for (id, ecosystem, root) in [
            ("rust:a", Ecosystem::Rust, "rust"),
            ("node:b", Ecosystem::Node, "node"),
        ] {
            graph.packages.insert(
                id.to_string(),
                PackageNode {
                    id: id.to_string(),
                    name: id.to_string(),
                    ecosystem,
                    root: root.to_string(),
                    dependencies: BTreeSet::new(),
                    targets: Vec::new(),
                    test_paths: Vec::new(),
                },
            );
        }
        assert!(select_from_graph(
            &graph,
            &["rust/lib.rs".to_string(), "node/index.ts".to_string()],
            MechanisticCacheStatusV1::Miss,
        )
        .is_err());
    }

    #[test]
    fn node_and_python_manifests_build_reverse_dependency_edges() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codex-team-graph-{nonce}"));
        fs::create_dir_all(root.join("node/core")).unwrap();
        fs::create_dir_all(root.join("node/app")).unwrap();
        fs::create_dir_all(root.join("python/core")).unwrap();
        fs::create_dir_all(root.join("python/app")).unwrap();
        fs::write(
            root.join("node/core/package.json"),
            r#"{"name":"@ca/core"}"#,
        )
        .unwrap();
        fs::write(
            root.join("node/app/package.json"),
            r#"{"name":"@ca/app","dependencies":{"@ca/core":"workspace:*"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("python/core/pyproject.toml"),
            "[project]\nname = \"ca-core\"\n",
        )
        .unwrap();
        fs::write(
            root.join("python/app/pyproject.toml"),
            "[project]\nname = \"ca-app\"\ndependencies = [\"ca-core>=1\"]\n",
        )
        .unwrap();
        let tracked = [
            "node/core/package.json",
            "node/core/src/index.ts",
            "node/app/package.json",
            "node/app/src/index.ts",
            "python/core/pyproject.toml",
            "python/core/ca_core.py",
            "python/app/pyproject.toml",
            "python/app/ca_app.py",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        let mut graph = VerificationGraph::default();
        build_node_graph(&root, &tracked, &mut graph).unwrap();
        build_python_graph(&root, &tracked, &mut graph).unwrap();

        let node = select_from_graph(
            &graph,
            &["node/core/src/index.ts".into()],
            MechanisticCacheStatusV1::Miss,
        )
        .unwrap();
        assert_eq!(node.owning_packages, vec!["@ca/core"]);
        assert_eq!(node.reverse_dependencies, vec!["@ca/app"]);
        let python = select_from_graph(
            &graph,
            &["python/core/ca_core.py".into()],
            MechanisticCacheStatusV1::Miss,
        )
        .unwrap();
        assert_eq!(python.owning_packages, vec!["ca-core"]);
        assert_eq!(python.reverse_dependencies, vec!["ca-app"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn swift_metadata_selects_owned_targets_and_tests() {
        let tracked = [
            "swift/Package.swift",
            "swift/Sources/Core/Cursor.swift",
            "swift/Tests/CoreTests/CursorTests.swift",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
        let mut graph = VerificationGraph::default();
        parse_swift_metadata(
            &serde_json::json!({
                "name": "Composer",
                "targets": [{"name": "Core"}, {"name": "CoreTests"}]
            }),
            "swift".into(),
            &tracked,
            &mut graph,
        )
        .unwrap();
        let selected = select_from_graph(
            &graph,
            &["swift/Sources/Core/Cursor.swift".into()],
            MechanisticCacheStatusV1::Miss,
        )
        .unwrap();
        assert_eq!(selected.owning_packages, vec!["Composer"]);
        assert!(selected
            .selected_targets
            .iter()
            .any(|target| target == "Composer:CoreTests"));
        assert_eq!(
            selected.relevant_test_paths,
            vec!["swift/Tests/CoreTests/CursorTests.swift"]
        );
    }
}
