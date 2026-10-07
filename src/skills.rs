use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_SKILL_BYTES: u64 = 128 * 1024;
pub const ACTIVE_SKILL_TOKEN_BUDGET: usize = 12_000;
pub const ACTIVATE_SKILL_TOOL_NAME: &str = "activate_skill";
const MAX_CATALOG_ENTRIES: usize = 64;
const MAX_CATALOG_CHARS: usize = 16 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 1024;
pub const MAX_SKILL_TREE_FILES: usize = 256;
pub const MAX_SKILL_TREE_DEPTH: usize = 8;
pub const MAX_SKILL_RESOURCE_BYTES: u64 = 1024 * 1024;
pub const MAX_SKILL_TREE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_SKILL_SUGGESTIONS: usize = 5;
const BM25_K1: f64 = 1.2;
const BM25_LENGTH_NORMALIZATION: f64 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillOrigin {
    Project,
    User,
    Compatibility,
}

impl std::fmt::Display for SkillOrigin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Compatibility => "compatibility",
        })
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceScope {
    Workspace,
    User,
    Admin,
    Bundled,
    Managed,
    Plugin,
    AutoDistill,
    Legacy,
}

impl SkillSourceScope {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::User => "user",
            Self::Admin => "admin",
            Self::Bundled => "bundled",
            Self::Managed => "managed",
            Self::Plugin => "plugin",
            Self::AutoDistill => "auto_distill",
            Self::Legacy => "legacy",
        }
    }

    fn legacy_origin(self) -> SkillOrigin {
        match self {
            Self::Workspace => SkillOrigin::Project,
            Self::User => SkillOrigin::User,
            Self::Admin
            | Self::Bundled
            | Self::Managed
            | Self::Plugin
            | Self::AutoDistill
            | Self::Legacy => SkillOrigin::Compatibility,
        }
    }
}

impl std::fmt::Display for SkillSourceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDiscoveryRoot {
    pub source_scope: SkillSourceScope,
    pub path: PathBuf,
    pub locator: String,
}

impl SkillDiscoveryRoot {
    pub fn new(
        source_scope: SkillSourceScope,
        path: impl Into<PathBuf>,
        locator: impl Into<String>,
    ) -> Self {
        Self {
            source_scope,
            path: path.into(),
            locator: locator.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDependency {
    pub dependency_type: String,
    pub value: String,
    pub description: Option<String>,
    pub transport: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SkillSettingValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Strings(Vec<String>),
}

impl SkillSettingValue {
    pub(crate) fn quoted(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "null".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillConfigDeclaration {
    pub key: String,
    pub description: Option<String>,
    pub default: Option<SkillSettingValue>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAvailabilityConditions {
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub requires_tools: Vec<String>,
    #[serde(default)]
    pub requires_toolsets: Vec<String>,
    #[serde(default)]
    pub fallback_for_tools: Vec<String>,
    #[serde(default)]
    pub fallback_for_toolsets: Vec<String>,
    #[serde(default)]
    pub config: Vec<SkillConfigDeclaration>,
    #[serde(default)]
    pub required_environment_variables: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAvailability {
    #[serde(default)]
    pub unavailable_reasons: Vec<String>,
    #[serde(default)]
    pub setup_required: Vec<String>,
    #[serde(default)]
    pub resolved_config: BTreeMap<String, SkillSettingValue>,
}

impl SkillAvailability {
    pub fn compatible(&self) -> bool {
        self.unavailable_reasons.is_empty()
    }

    pub fn runnable(&self) -> bool {
        self.compatible() && self.setup_required.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInstanceMetadata {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub related_skills: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata_keys: Vec<String>,
    pub allowed_tools: Option<String>,
    #[serde(default = "default_true")]
    pub allow_implicit_invocation: bool,
    #[serde(default)]
    pub dependencies: Vec<SkillDependency>,
    #[serde(default)]
    pub conditions: SkillAvailabilityConditions,
}

fn default_true() -> bool {
    true
}

fn normalized_platform(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "darwin" | "mac" | "macos" | "osx" => "macos".into(),
        "win" | "win32" | "windows" => "windows".into(),
        "linux" => "linux".into(),
        "freebsd" => "freebsd".into(),
        other => other.into(),
    }
}

fn ensure_instance_available(instance: &SkillInstance) -> Result<(), SkillError> {
    if !instance.availability.unavailable_reasons.is_empty() {
        return Err(SkillError::Unavailable(format!(
            "{} ({})",
            instance.metadata.name,
            instance.availability.unavailable_reasons.join("; ")
        )));
    }
    if !instance.availability.setup_required.is_empty() {
        return Err(SkillError::ConfigurationRequired(format!(
            "{} needs {}; use /skills config {}",
            instance.metadata.name,
            instance.availability.setup_required.join(", "),
            instance.qualified_name
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    pub relative_path: PathBuf,
    pub size: u64,
    pub mode: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInstance {
    pub instance_id: String,
    pub qualified_name: String,
    pub source_scope: SkillSourceScope,
    pub locator: String,
    pub content_digest: String,
    pub metadata: SkillInstanceMetadata,
    pub files: Vec<SkillFile>,
    pub enabled: bool,
    #[serde(default)]
    pub availability: SkillAvailability,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillManageAction {
    Create,
    Patch,
    Edit,
    Delete,
    WriteFile,
    RemoveFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillManageFile {
    pub path: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillManageRequest {
    pub action: SkillManageAction,
    /// Exact instance ID/qualified name for existing skills, or the exact new
    /// skill name for `create`.
    pub selector: String,
    /// Exact current full-tree digest, or the literal `absent` for `create`.
    pub expected_full_tree_digest: String,
    #[serde(default)]
    pub scope: Option<SkillSourceScope>,
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub old_text: Option<String>,
    #[serde(default)]
    pub new_text: Option<String>,
    #[serde(default)]
    pub files: Vec<SkillManageFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillManageReceipt {
    pub action: SkillManageAction,
    pub name: String,
    pub instance_id: Option<String>,
    pub full_tree_digest: Option<String>,
    pub journal_id: uuid::Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoad {
    pub instance_id: String,
    pub content_digest: String,
    pub requested_resources: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillLoadSet {
    loads: BTreeMap<String, SkillLoad>,
}

impl SkillLoadSet {
    pub fn iter(&self) -> impl Iterator<Item = &SkillLoad> {
        self.loads.values()
    }

    pub fn insert(&mut self, load: SkillLoad) {
        self.loads.insert(load.instance_id.clone(), load);
    }

    pub fn clear(&mut self) {
        self.loads.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.loads.is_empty()
    }

    pub fn len(&self) -> usize {
        self.loads.len()
    }
}

#[derive(Debug, Clone)]
pub struct SkillRoots {
    pub project: Option<PathBuf>,
    pub project_boundary: Option<PathBuf>,
    pub user: Option<PathBuf>,
    pub compatibility: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub id: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub path: PathBuf,
    pub sha256: String,
    pub origin: SkillOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillMatchKind {
    ExactId,
    ExactAlias,
    Related,
}

impl SkillMatchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ExactId => "exact ID",
            Self::ExactAlias => "exact alias",
            Self::Related => "related",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSuggestion {
    pub id: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub origin: SkillOrigin,
    pub active: bool,
    pub match_kind: SkillMatchKind,
    pub matched_alias: Option<String>,
    /// Deterministic display-only relevance in basis points (1..=1000).
    pub relevance: u16,
}

#[derive(Debug)]
struct RankedSuggestion {
    suggestion: SkillSuggestion,
    raw_score: f64,
    coverage: u16,
}

#[derive(Debug, Clone, Default)]
struct SkillSearchIndex {
    documents: BTreeMap<String, Vec<String>>,
    document_frequency: HashMap<String, usize>,
    average_length: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveSkillRef {
    pub id: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentSkillRef {
    pub version: u32,
    pub instance_id: String,
    pub qualified_name: String,
    pub source_locator: String,
    pub full_digest: String,
}

impl PersistentSkillRef {
    pub fn from_instance(instance: &SkillInstance) -> Self {
        Self {
            version: 1,
            instance_id: instance.instance_id.clone(),
            qualified_name: instance.qualified_name.clone(),
            source_locator: instance.locator.clone(),
            full_digest: instance.content_digest.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveSkill {
    reference: ActiveSkillRef,
    body: String,
    estimated_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct SkillRegistry {
    roots: SkillRoots,
    disabled: HashSet<String>,
    entries: BTreeMap<String, SkillMetadata>,
    search_index: SkillSearchIndex,
    active: Vec<ActiveSkill>,
    diagnostics: Vec<String>,
    instances: BTreeMap<String, SkillInstance>,
    instance_sources: HashMap<String, SkillInstanceSource>,
    portable_roots: Option<Vec<SkillDiscoveryRoot>>,
    catalog_hidden: HashSet<String>,
    refinement_turn: Option<SkillRefinementTurn>,
}

#[derive(Debug, Clone, Default)]
struct SkillRefinementTurn {
    pins: BTreeMap<String, String>,
    attempts: BTreeMap<String, usize>,
    committed: HashSet<String>,
    evidence_observed: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillError {
    #[error("unknown skill ID: {0}")]
    Unknown(String),
    #[error("skill is disabled: {0}")]
    Disabled(String),
    #[error("skill is unavailable: {0}")]
    Unavailable(String),
    #[error("skill setup is required: {0}")]
    ConfigurationRequired(String),
    #[error("skill changed since discovery: {0}")]
    Changed(String),
    #[error("ambiguous skill {selector}; choose one of: {choices}")]
    Ambiguous { selector: String, choices: String },
    #[error("active skill content exceeds the {ACTIVE_SKILL_TOKEN_BUDGET}-token budget")]
    BudgetExceeded,
    #[error("invalid skill: {0}")]
    Invalid(String),
    #[error("skill I/O failed: {0}")]
    Io(String),
}

#[derive(Debug)]
struct ParsedSkill {
    id: String,
    description: String,
    aliases: Vec<String>,
    body: String,
}

#[derive(Debug)]
struct Candidate {
    metadata: SkillMetadata,
}

#[derive(Debug, Clone)]
struct SkillInstanceSource {
    skill_root: PathBuf,
    skill_md_sha256: String,
}

#[derive(Debug)]
struct PortableParsedSkill {
    metadata: SkillInstanceMetadata,
    body: String,
}

impl SkillRegistry {
    pub(crate) fn instance_path(&self, instance_id: &str) -> Option<&Path> {
        self.instance_sources
            .get(instance_id)
            .map(|source| source.skill_root.as_path())
    }

    pub fn discover(roots: SkillRoots, disabled: &[String]) -> Self {
        let disabled: HashSet<String> = disabled.iter().map(|id| id.to_ascii_lowercase()).collect();
        let mut registry = Self {
            roots: roots.clone(),
            disabled,
            entries: BTreeMap::new(),
            search_index: SkillSearchIndex::default(),
            active: Vec::new(),
            diagnostics: Vec::new(),
            instances: BTreeMap::new(),
            instance_sources: HashMap::new(),
            portable_roots: None,
            catalog_hidden: HashSet::new(),
            refinement_turn: None,
        };
        let tiers = [
            (
                SkillOrigin::Project,
                roots
                    .project
                    .into_iter()
                    .map(|root| (root, roots.project_boundary.clone()))
                    .collect::<Vec<_>>(),
            ),
            (
                SkillOrigin::User,
                roots
                    .user
                    .into_iter()
                    .map(|root| (root, None))
                    .collect::<Vec<_>>(),
            ),
            (
                SkillOrigin::Compatibility,
                roots
                    .compatibility
                    .into_iter()
                    .map(|root| (root, None))
                    .collect(),
            ),
        ];
        let mut selected_identities: HashMap<String, String> = HashMap::new();
        let mut canonical_files = HashSet::new();
        for (origin, tier_roots) in tiers {
            let mut candidates = Vec::new();
            for (root, boundary) in tier_roots {
                collect_candidates(
                    &root,
                    boundary.as_deref(),
                    origin,
                    &registry.disabled,
                    &mut canonical_files,
                    &mut candidates,
                    &mut registry.diagnostics,
                );
            }
            candidates.sort_by(|left, right| left.metadata.path.cmp(&right.metadata.path));
            let mut identity_counts: HashMap<String, usize> = HashMap::new();
            for candidate in &candidates {
                for identity in identities(&candidate.metadata) {
                    *identity_counts.entry(identity).or_default() += 1;
                }
            }
            for candidate in candidates {
                let candidate_identities = identities(&candidate.metadata);
                if let Some(identity) = candidate_identities
                    .iter()
                    .find(|identity| identity_counts.get(*identity).copied().unwrap_or(0) > 1)
                {
                    registry.diagnostics.push(format!(
                        "{}: duplicate {origin} skill identity {identity}",
                        candidate.metadata.path.display()
                    ));
                    continue;
                }
                if let Some((identity, owner)) = candidate_identities.iter().find_map(|identity| {
                    selected_identities
                        .get(identity)
                        .map(|owner| (identity, owner))
                }) {
                    registry.diagnostics.push(format!(
                        "{}: {origin} skill identity {identity} is shadowed by {owner}",
                        candidate.metadata.path.display()
                    ));
                    continue;
                }
                let id = candidate.metadata.id.clone();
                for identity in candidate_identities {
                    selected_identities.insert(identity, id.clone());
                }
                registry.entries.insert(id, candidate.metadata);
            }
        }
        registry.rebuild_search_index();
        registry
    }

    pub fn discover_portable(roots: Vec<SkillDiscoveryRoot>, disabled: &[String]) -> Self {
        let disabled = disabled
            .iter()
            .map(|name| name.trim().to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let mut registry = Self {
            roots: SkillRoots {
                project: None,
                project_boundary: None,
                user: None,
                compatibility: Vec::new(),
            },
            disabled: disabled.clone(),
            entries: BTreeMap::new(),
            search_index: SkillSearchIndex::default(),
            active: Vec::new(),
            diagnostics: Vec::new(),
            instances: BTreeMap::new(),
            instance_sources: HashMap::new(),
            portable_roots: Some(roots.clone()),
            catalog_hidden: HashSet::new(),
            refinement_turn: None,
        };
        recover_portable_skill_journals(&roots, &mut registry.diagnostics);
        collect_portable_instances(
            &roots,
            &disabled,
            &mut registry.instances,
            &mut registry.instance_sources,
            &mut registry.diagnostics,
        );

        registry.rebuild_portable_entries();
        registry
    }

    pub fn instances(&self) -> impl Iterator<Item = &SkillInstance> {
        self.instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
    }

    pub fn unavailable_instances(&self) -> impl Iterator<Item = &SkillInstance> {
        self.instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
            .filter(|instance| !instance.availability.runnable())
    }

    /// Remove workspace-owned skills from automatic catalogs while retaining
    /// them for an explicit user invocation by exact selector.
    pub fn hide_workspace_context(&mut self) {
        self.catalog_hidden = self
            .instances
            .values()
            .filter(|instance| {
                instance.source_scope == SkillSourceScope::Workspace
                    || instance.locator.starts_with("workspace-ca-")
            })
            .map(|instance| instance.instance_id.clone())
            .collect();
        self.rebuild_portable_entries();
    }

    /// Recompute host-owned availability without granting any tool or reading
    /// any environment-variable value. Workspace values take precedence over
    /// profile values; declared defaults are used last.
    pub fn evaluate_availability(
        &mut self,
        available_tools: &BTreeSet<String>,
        available_toolsets: &BTreeSet<String>,
        profile_config: &BTreeMap<String, SkillSettingValue>,
        workspace_config: &BTreeMap<String, SkillSettingValue>,
    ) {
        for instance in self.instances.values_mut() {
            let conditions = &instance.metadata.conditions;
            let mut availability = SkillAvailability::default();
            let platform = normalized_platform(std::env::consts::OS);
            if !conditions.platforms.is_empty()
                && !conditions
                    .platforms
                    .iter()
                    .any(|candidate| normalized_platform(candidate) == platform)
            {
                availability.unavailable_reasons.push(format!(
                    "platform {platform} is not in [{}]",
                    conditions.platforms.join(", ")
                ));
            }
            let missing_tools = conditions
                .requires_tools
                .iter()
                .filter(|tool| !available_tools.contains(&tool.to_ascii_lowercase()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing_tools.is_empty() {
                availability
                    .unavailable_reasons
                    .push(format!("missing tools: {}", missing_tools.join(", ")));
            }
            let missing_toolsets = conditions
                .requires_toolsets
                .iter()
                .filter(|toolset| !available_toolsets.contains(&toolset.to_ascii_lowercase()))
                .cloned()
                .collect::<Vec<_>>();
            if !missing_toolsets.is_empty() {
                availability
                    .unavailable_reasons
                    .push(format!("missing toolsets: {}", missing_toolsets.join(", ")));
            }
            if !conditions.fallback_for_tools.is_empty()
                && conditions
                    .fallback_for_tools
                    .iter()
                    .all(|tool| available_tools.contains(&tool.to_ascii_lowercase()))
            {
                availability
                    .unavailable_reasons
                    .push("fallback is not needed because its tools are available".into());
            }
            if !conditions.fallback_for_toolsets.is_empty()
                && conditions
                    .fallback_for_toolsets
                    .iter()
                    .all(|toolset| available_toolsets.contains(&toolset.to_ascii_lowercase()))
            {
                availability
                    .unavailable_reasons
                    .push("fallback is not needed because its toolsets are available".into());
            }
            let missing_environment = conditions
                .required_environment_variables
                .iter()
                .filter(|name| crate::run_environment::var_os(name).is_none())
                .cloned()
                .collect::<Vec<_>>();
            if !missing_environment.is_empty() {
                availability.unavailable_reasons.push(format!(
                    "missing required environment variables: {}",
                    missing_environment.join(", ")
                ));
            }
            for declaration in &conditions.config {
                let qualified = format!("{}.{}", instance.qualified_name, declaration.key);
                let legacy_qualified = format!("{}.{}", instance.metadata.name, declaration.key);
                let value = workspace_config
                    .get(&qualified)
                    .or_else(|| workspace_config.get(&legacy_qualified))
                    .or_else(|| profile_config.get(&qualified))
                    .or_else(|| profile_config.get(&legacy_qualified))
                    .or(declaration.default.as_ref());
                if let Some(value) = value {
                    availability
                        .resolved_config
                        .insert(declaration.key.clone(), value.clone());
                } else if declaration.required {
                    availability.setup_required.push(declaration.key.clone());
                }
            }
            instance.availability = availability;
        }
        self.rebuild_portable_entries();
    }

    fn rebuild_portable_entries(&mut self) {
        if self.portable_roots.is_none() {
            self.rebuild_search_index();
            return;
        }
        self.entries.clear();
        let mut by_name = BTreeMap::<String, Vec<String>>::new();
        for instance in self
            .instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
        {
            by_name
                .entry(instance.metadata.name.clone())
                .or_default()
                .push(instance.instance_id.clone());
        }
        for (name, ids) in by_name {
            if ids.len() != 1 {
                continue;
            }
            let instance = &self.instances[&ids[0]];
            if !instance.enabled || !instance.availability.compatible() {
                continue;
            }
            let source = &self.instance_sources[&ids[0]];
            self.entries.insert(
                name.clone(),
                SkillMetadata {
                    id: name,
                    description: instance.metadata.description.clone(),
                    aliases: instance.metadata.aliases.clone(),
                    path: source.skill_root.join("SKILL.md"),
                    sha256: source.skill_md_sha256.clone(),
                    origin: instance.source_scope.legacy_origin(),
                },
            );
        }
        self.rebuild_search_index();
    }

    pub fn start_refinement_turn(&mut self, loads: &SkillLoadSet, user_correction: bool) {
        self.refinement_turn = Some(SkillRefinementTurn {
            pins: loads
                .iter()
                .map(|load| (load.instance_id.clone(), load.content_digest.clone()))
                .collect(),
            evidence_observed: user_correction,
            ..SkillRefinementTurn::default()
        });
    }

    pub fn note_refinement_failure_or_recovery(&mut self) {
        if let Some(turn) = &mut self.refinement_turn {
            if !turn.pins.is_empty() {
                turn.evidence_observed = true;
            }
        }
    }

    pub fn activation_selectors(&self) -> Vec<String> {
        if self.instances.is_empty() {
            return self.entries.keys().cloned().collect();
        }
        let mut selectors = self
            .instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
            .filter(|instance| instance.enabled && instance.availability.compatible())
            .flat_map(|instance| {
                [
                    instance.qualified_name.clone(),
                    instance.instance_id.clone(),
                ]
            })
            .collect::<Vec<_>>();
        selectors.sort();
        selectors.dedup();
        selectors
    }

    pub fn preflight_activation(&self, selector: &str) -> Result<(), SkillError> {
        if !self.instances.is_empty() {
            self.resolve_instance(selector, true).map(|_| ())
        } else {
            let id = selector.trim().to_ascii_lowercase();
            if self.disabled.contains(&id) {
                Err(SkillError::Disabled(id))
            } else if self.entries.contains_key(&id) {
                Ok(())
            } else {
                Err(SkillError::Unknown(id))
            }
        }
    }

    pub fn preflight_view(&self, instance_id: &str) -> Result<(), SkillError> {
        let instance = self
            .instances
            .get(instance_id.trim())
            .filter(|instance| instance.enabled)
            .ok_or_else(|| SkillError::Unknown(instance_id.trim().to_string()))?;
        ensure_instance_available(instance)
    }

    pub fn viewable_instance_ids(&self) -> Vec<String> {
        self.instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
            .filter(|instance| instance.enabled && instance.availability.compatible())
            .map(|instance| instance.instance_id.clone())
            .collect()
    }

    pub fn set_instance_enabled(
        &mut self,
        instance_id: &str,
        full_digest: &str,
        enabled: bool,
    ) -> Result<(), SkillError> {
        let instance = self
            .instances
            .get_mut(instance_id)
            .ok_or_else(|| SkillError::Unknown(instance_id.to_string()))?;
        if instance.content_digest != full_digest {
            return Err(SkillError::Changed(instance_id.to_string()));
        }
        instance.enabled = enabled;
        Ok(())
    }

    pub fn apply_exact_state_file(&mut self, path: &Path) {
        if !path.exists() {
            return;
        }
        let result = (|| -> Result<(), SkillError> {
            let metadata = std::fs::symlink_metadata(path)
                .map_err(|error| SkillError::Io(error.to_string()))?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 1024 * 1024
            {
                return Err(SkillError::Invalid(
                    "exact skill state must be a bounded regular file".into(),
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(
                &std::fs::read(path).map_err(|error| SkillError::Io(error.to_string()))?,
            )
            .map_err(|error| SkillError::Invalid(error.to_string()))?;
            let records = value
                .get("records")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| SkillError::Invalid("exact skill state has no records".into()))?;
            for (id, record) in records {
                let digest = record
                    .get("digest")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| SkillError::Invalid("exact record has no digest".into()))?;
                let enabled = record
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| {
                        SkillError::Invalid("exact record has no enabled flag".into())
                    })?;
                if let Some(instance) = self.instances.get_mut(id) {
                    if instance.content_digest == digest {
                        instance.enabled = enabled;
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.diagnostics.push(format!("skill state: {error}"));
        }
    }

    pub fn resolve_instance(
        &self,
        selector: &str,
        explicit: bool,
    ) -> Result<&SkillInstance, SkillError> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Err(SkillError::Unknown("<empty>".into()));
        }
        if let Some(instance) = self.instances.get(selector).or_else(|| {
            self.instances
                .values()
                .find(|instance| instance.qualified_name == selector)
        }) {
            if !instance.enabled {
                return Err(SkillError::Disabled(instance.metadata.name.clone()));
            }
            ensure_instance_available(instance)?;
            if !explicit && !instance.metadata.allow_implicit_invocation {
                return Err(SkillError::Unknown(selector.into()));
            }
            return Ok(instance);
        }
        let named = self
            .instances
            .values()
            .filter(|instance| instance.metadata.name == selector)
            .collect::<Vec<_>>();
        if let [instance] = named.as_slice() {
            if !instance.enabled {
                return Err(SkillError::Disabled(instance.metadata.name.clone()));
            }
            ensure_instance_available(instance)?;
            if !explicit && !instance.metadata.allow_implicit_invocation {
                return Err(SkillError::Unknown(selector.into()));
            }
            return Ok(instance);
        }
        let mut matches = named
            .into_iter()
            .filter(|instance| instance.enabled)
            .filter(|instance| instance.availability.runnable())
            .filter(|instance| explicit || instance.metadata.allow_implicit_invocation)
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
        match matches.as_slice() {
            [] => Err(SkillError::Unknown(selector.into())),
            [instance] => Ok(*instance),
            _ => Err(SkillError::Ambiguous {
                selector: selector.into(),
                choices: matches
                    .iter()
                    .map(|instance| instance.qualified_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    pub fn inspect_instance(&self, selector: &str) -> Result<&SkillInstance, SkillError> {
        let selector = selector.trim();
        if let Some(instance) = self.instances.get(selector).or_else(|| {
            self.instances
                .values()
                .find(|instance| instance.qualified_name == selector)
        }) {
            return Ok(instance);
        }
        let matches = self
            .instances
            .values()
            .filter(|instance| instance.metadata.name == selector)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Err(SkillError::Unknown(selector.into())),
            [instance] => Ok(*instance),
            _ => Err(SkillError::Ambiguous {
                selector: selector.into(),
                choices: matches
                    .iter()
                    .map(|instance| instance.qualified_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    pub fn skill_view(&self, instance_id: &str, relative_path: &str) -> Result<String, SkillError> {
        let instance = self
            .instances
            .get(instance_id)
            .ok_or_else(|| SkillError::Unknown(instance_id.into()))?;
        if !instance.enabled {
            return Err(SkillError::Disabled(instance.metadata.name.clone()));
        }
        ensure_instance_available(instance)?;
        let source = self
            .instance_sources
            .get(instance_id)
            .ok_or_else(|| SkillError::Changed(instance_id.into()))?;
        let refreshed = load_portable_skill_tree(
            &source.skill_root,
            instance.source_scope,
            &instance.locator,
            instance.enabled,
        )?;
        if refreshed.instance.content_digest != instance.content_digest
            || refreshed.instance.instance_id != instance.instance_id
        {
            return Err(SkillError::Changed(instance_id.into()));
        }
        let relative = validate_skill_relative_path(relative_path)?;
        if !refreshed
            .instance
            .files
            .iter()
            .any(|file| file.relative_path == relative)
        {
            return Err(SkillError::Invalid(format!(
                "resource is not in the pinned skill inventory: {}",
                relative.display()
            )));
        }
        read_skill_resource(&source.skill_root, &relative)
    }

    /// Atomically mutate one complete portable skill tree. This operation only
    /// changes files; it never executes scripts, installs dependencies, changes
    /// tool grants, or bypasses the caller's normal approval boundary.
    pub fn skill_manage(
        &mut self,
        request: SkillManageRequest,
    ) -> Result<SkillManageReceipt, SkillError> {
        let refinement_pin = if request.action == SkillManageAction::Create {
            None
        } else {
            self.instances
                .get(request.selector.trim())
                .or_else(|| {
                    self.instances
                        .values()
                        .find(|instance| instance.qualified_name == request.selector.trim())
                })
                .map(|instance| instance.instance_id.clone())
        };
        if let (Some(turn), Some(instance_id)) = (&mut self.refinement_turn, &refinement_pin) {
            if !turn.evidence_observed {
                return Err(SkillError::Invalid(
                    "automatic refinement requires a concrete failure, recovery, or user correction in this task"
                        .into(),
                ));
            }
            let expected = turn.pins.get(instance_id).ok_or_else(|| {
                SkillError::Invalid(
                    "automatic refinement is limited to a skill pinned for this task".into(),
                )
            })?;
            if expected != &request.expected_full_tree_digest {
                return Err(SkillError::Changed(request.selector));
            }
            if turn.committed.contains(instance_id) {
                return Err(SkillError::Invalid(
                    "only one skill mutation may commit per user turn".into(),
                ));
            }
            let attempts = turn.attempts.entry(instance_id.clone()).or_default();
            *attempts += 1;
            if *attempts > 2 {
                return Err(SkillError::Invalid(
                    "skill refinement exhausted its two structured repair attempts".into(),
                ));
            }
        }
        let journal_id = uuid::Uuid::new_v4();
        let (name, target, discovery_root, source_scope, prior_instance_id) = if request.action
            == SkillManageAction::Create
        {
            if request.expected_full_tree_digest != "absent" {
                return Err(SkillError::Changed(request.selector));
            }
            validate_portable_name(&request.selector)?;
            if crate::tui::commands::find_command(&request.selector).is_some() {
                return Err(SkillError::Invalid(
                    "skill name collides with a built-in command".into(),
                ));
            }
            let scope = request.scope.ok_or_else(|| {
                SkillError::Invalid("create requires an exact writable scope".into())
            })?;
            if !matches!(
                scope,
                SkillSourceScope::Workspace
                    | SkillSourceScope::User
                    | SkillSourceScope::AutoDistill
            ) {
                return Err(SkillError::Invalid(
                    "create scope is read-only or externally managed".into(),
                ));
            }
            let root = self
                .portable_roots
                .as_ref()
                .and_then(|roots| roots.iter().find(|root| root.source_scope == scope))
                .cloned()
                .ok_or_else(|| SkillError::Invalid("requested skill root is unavailable".into()))?;
            ensure_writable_skill_root(&root.path)?;
            let target = root.path.join(&request.selector);
            if target.exists() {
                return Err(SkillError::Changed(request.selector));
            }
            (request.selector.clone(), target, root.path, scope, None)
        } else {
            if request.scope.is_some() {
                return Err(SkillError::Invalid(
                    "scope is accepted only when creating a skill".into(),
                ));
            }
            let instance = self.resolve_instance(&request.selector, true)?;
            if request.selector != instance.instance_id
                && request.selector != instance.qualified_name
            {
                return Err(SkillError::Invalid(
                    "existing skill mutations require an exact instance ID or qualified name"
                        .into(),
                ));
            }
            if instance.content_digest != request.expected_full_tree_digest {
                return Err(SkillError::Changed(request.selector));
            }
            if !matches!(
                instance.source_scope,
                SkillSourceScope::Workspace
                    | SkillSourceScope::User
                    | SkillSourceScope::AutoDistill
                    | SkillSourceScope::Legacy
            ) {
                return Err(SkillError::Invalid(
                    "skill is read-only, plugin-managed, or administrator-owned".into(),
                ));
            }
            let source = self
                .instance_sources
                .get(&instance.instance_id)
                .ok_or_else(|| SkillError::Changed(instance.instance_id.clone()))?;
            let root = self
                .portable_roots
                .as_ref()
                .and_then(|roots| {
                    roots
                        .iter()
                        .filter(|root| {
                            fs::canonicalize(&root.path)
                                .is_ok_and(|path| source.skill_root.starts_with(path))
                        })
                        .max_by_key(|root| root.path.components().count())
                })
                .ok_or_else(|| SkillError::Invalid("skill discovery root is unavailable".into()))?;
            (
                instance.metadata.name.clone(),
                source.skill_root.clone(),
                root.path.clone(),
                instance.source_scope,
                Some(instance.instance_id.clone()),
            )
        };

        if request.action == SkillManageAction::Delete {
            if !request.files.is_empty()
                || request.relative_path.is_some()
                || request.content.is_some()
                || request.old_text.is_some()
                || request.new_text.is_some()
            {
                return Err(SkillError::Invalid(
                    "delete accepts only selector and expected digest".into(),
                ));
            }
            let backup = skill_journal_backup(&discovery_root, journal_id, &name)?;
            write_skill_journal(
                &discovery_root,
                journal_id,
                "prepared",
                request.action,
                &target,
                Some(&backup),
                &request.expected_full_tree_digest,
                None,
            )?;
            fs::rename(&target, &backup).map_err(|error| SkillError::Io(error.to_string()))?;
            write_skill_journal(
                &discovery_root,
                journal_id,
                "committed",
                request.action,
                &target,
                Some(&backup),
                &request.expected_full_tree_digest,
                None,
            )?;
            self.refresh_portable();
            if let (Some(turn), Some(instance_id)) = (&mut self.refinement_turn, refinement_pin) {
                turn.committed.insert(instance_id);
            }
            return Ok(SkillManageReceipt {
                action: request.action,
                name,
                instance_id: prior_instance_id,
                full_tree_digest: Some(request.expected_full_tree_digest),
                journal_id,
            });
        }

        let parent = target
            .parent()
            .ok_or_else(|| SkillError::Invalid("skill target has no parent".into()))?;
        fs::create_dir_all(parent).map_err(|error| SkillError::Io(error.to_string()))?;
        let staging = parent.join(format!(".ca-skill-{journal_id}.tmp"));
        if staging.exists() {
            return Err(SkillError::Changed(request.selector));
        }
        fs::create_dir(&staging).map_err(|error| SkillError::Io(error.to_string()))?;
        let operation = (|| -> Result<(), SkillError> {
            if request.action != SkillManageAction::Create {
                copy_skill_tree_text(&target, &staging)?;
            }
            apply_skill_tree_request(&staging, &request)?;
            validate_staged_skill_tree(&staging, source_scope, &name)?;
            Ok(())
        })();
        if let Err(error) = operation {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let loaded = load_portable_skill_tree(&staging, SkillSourceScope::Legacy, "staged", true)?;
        let new_digest = loaded.instance.content_digest.clone();
        let backup = (request.action != SkillManageAction::Create)
            .then(|| skill_journal_backup(&discovery_root, journal_id, &name))
            .transpose()?;
        write_skill_journal(
            &discovery_root,
            journal_id,
            "prepared",
            request.action,
            &target,
            backup.as_deref(),
            &request.expected_full_tree_digest,
            Some(&new_digest),
        )?;
        if let Some(backup) = &backup {
            fs::rename(&target, backup).map_err(|error| SkillError::Io(error.to_string()))?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if let Some(backup) = &backup {
                let _ = fs::rename(backup, &target);
            }
            return Err(SkillError::Io(error.to_string()));
        }
        write_skill_journal(
            &discovery_root,
            journal_id,
            "committed",
            request.action,
            &target,
            backup.as_deref(),
            &request.expected_full_tree_digest,
            Some(&new_digest),
        )?;
        self.refresh_portable();
        let instance = self
            .instances
            .values()
            .find(|instance| {
                instance.metadata.name == name && instance.content_digest == new_digest
            })
            .ok_or_else(|| SkillError::Invalid("mutated skill failed registry refresh".into()))?;
        if let (Some(turn), Some(instance_id)) = (&mut self.refinement_turn, refinement_pin) {
            turn.committed.insert(instance_id);
        }
        Ok(SkillManageReceipt {
            action: request.action,
            name,
            instance_id: Some(instance.instance_id.clone()),
            full_tree_digest: Some(new_digest),
            journal_id,
        })
    }

    pub(crate) fn refresh_portable(&mut self) {
        let Some(roots) = self.portable_roots.clone() else {
            return;
        };
        let disabled = self.disabled.iter().cloned().collect::<Vec<_>>();
        let refinement_turn = self.refinement_turn.take();
        *self = Self::discover_portable(roots, &disabled);
        self.refinement_turn = refinement_turn;
    }

    pub fn load_for_task(
        &self,
        selector: &str,
        explicit: bool,
        loads: &mut SkillLoadSet,
    ) -> Result<(), SkillError> {
        let instance = self.resolve_instance(selector, explicit)?;
        let source = self
            .instance_sources
            .get(&instance.instance_id)
            .ok_or_else(|| SkillError::Changed(instance.instance_id.clone()))?;
        let refreshed = load_portable_skill_tree(
            &source.skill_root,
            instance.source_scope,
            &instance.locator,
            instance.enabled,
        )?;
        if refreshed.instance.instance_id != instance.instance_id
            || refreshed.instance.content_digest != instance.content_digest
        {
            return Err(SkillError::Changed(instance.instance_id.clone()));
        }
        loads.insert(SkillLoad {
            instance_id: instance.instance_id.clone(),
            content_digest: instance.content_digest.clone(),
            requested_resources: Vec::new(),
        });
        Ok(())
    }

    pub fn task_prompt(&self, loads: &SkillLoadSet) -> Result<String, SkillError> {
        let mut output = String::new();
        let mut estimated_tokens = 0usize;
        for load in loads.iter() {
            let instance = resolve_pinned_instance(
                self.instances.values(),
                &load.instance_id,
                &load.content_digest,
            )?;
            let source = self
                .instance_sources
                .get(&instance.instance_id)
                .ok_or_else(|| SkillError::Changed(instance.instance_id.clone()))?;
            for dependency in &instance.metadata.dependencies {
                if matches!(
                    dependency.dependency_type.to_ascii_lowercase().as_str(),
                    "binary" | "command" | "cli" | "executable"
                ) && !runtime_binary_available(&dependency.value)
                {
                    return Err(SkillError::Invalid(format!(
                        "skill {} requires missing runtime dependency {}; CA did not install it",
                        instance.metadata.name, dependency.value
                    )));
                }
            }
            let refreshed = load_portable_skill_tree(
                &source.skill_root,
                instance.source_scope,
                &instance.locator,
                instance.enabled,
            )?;
            if refreshed.instance.instance_id != instance.instance_id {
                return Err(SkillError::Changed(instance.instance_id.clone()));
            }
            estimated_tokens = estimated_tokens.saturating_add(estimate_tokens(&refreshed.body));
            if estimated_tokens > ACTIVE_SKILL_TOKEN_BUDGET {
                return Err(SkillError::BudgetExceeded);
            }
            output.push_str(&format!(
                "\n## Task skill: {}\n<skill-instructions instance_id=\"{}\" digest=\"{}\">\n{}\n</skill-instructions>\n",
                instance.qualified_name,
                instance.instance_id,
                instance.content_digest,
                refreshed.body
            ));
            if !instance.availability.resolved_config.is_empty() {
                output.push_str("<skill-settings untrusted-data=\"true\">\n");
                for (key, value) in &instance.availability.resolved_config {
                    output.push_str(&format!("{}={}\n", key, value.quoted()));
                }
                output.push_str("</skill-settings>\n");
            }
        }
        Ok(output)
    }

    pub fn portable_catalog_prompt(&self, model_context_tokens: Option<u64>) -> String {
        let context_budget = model_context_tokens
            .map(|tokens| (tokens / 50) as usize)
            .unwrap_or(8_000);
        let budget = 8_000usize.min(context_budget);
        let mut output = String::from(
            "## Available portable skills (metadata only)\n\
             When a skill is relevant to the current request, use activate_skill with its exact qualified name before proceeding. Activation is session-scoped and grants no tool authority.\n",
        );
        let enabled = self
            .instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
            .filter(|instance| instance.enabled && instance.availability.compatible())
            .count();
        let mut included = 0usize;
        for instance in self
            .instances
            .values()
            .filter(|instance| !self.catalog_hidden.contains(&instance.instance_id))
            .filter(|instance| instance.enabled && instance.availability.compatible())
        {
            let category = instance
                .metadata
                .category
                .as_ref()
                .map(|category| format!("; category: {category}"))
                .unwrap_or_default();
            let related = if instance.metadata.related_skills.is_empty() {
                String::new()
            } else {
                format!("; related: {}", instance.metadata.related_skills.join(", "))
            };
            let line = format!(
                "- {} [{}]: {}{}{}\n",
                instance.qualified_name,
                instance.instance_id,
                instance.metadata.description,
                category,
                related
            );
            if estimate_tokens(&(output.clone() + &line)) > budget {
                break;
            }
            output.push_str(&line);
            included += 1;
        }
        if included < enabled {
            output.push_str(&format!(
                "- {} skill(s) omitted by metadata budget\n",
                enabled - included
            ));
        }
        output
    }

    pub fn empty() -> Self {
        Self::discover(
            SkillRoots {
                project: None,
                project_boundary: None,
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        )
    }

    pub fn entries(&self) -> impl Iterator<Item = &SkillMetadata> {
        self.entries.values()
    }

    pub fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }

    pub fn search(&self, query: &str) -> Vec<&SkillMetadata> {
        let query = query.trim().to_ascii_lowercase();
        self.entries
            .values()
            .filter(|skill| {
                query.is_empty()
                    || skill.id.contains(&query)
                    || skill.description.to_ascii_lowercase().contains(&query)
                    || skill.aliases.iter().any(|alias| alias.contains(&query))
            })
            .collect()
    }

    pub fn suggest(&self, query: &str, limit: usize) -> Vec<SkillSuggestion> {
        let limit = limit.min(MAX_SKILL_SUGGESTIONS);
        if limit == 0 {
            return Vec::new();
        }
        let normalized_query = query.trim().to_lowercase();
        if normalized_query.is_empty() {
            return Vec::new();
        }
        let query_terms = unique_search_terms(&normalized_query);
        let mut ranked = Vec::new();
        for skill in self.entries.values() {
            let Some(document) = self.search_index.documents.get(&skill.id) else {
                continue;
            };
            let matched_alias = skill
                .aliases
                .iter()
                .find(|alias| mentions_identity(&normalized_query, alias))
                .cloned();
            let match_kind = if mentions_identity(&normalized_query, &skill.id) {
                Some(SkillMatchKind::ExactId)
            } else if matched_alias.is_some() {
                Some(SkillMatchKind::ExactAlias)
            } else {
                None
            };
            let mut raw_score = 0.0;
            let mut matched_terms = 0usize;
            for term in &query_terms {
                let frequency = document
                    .iter()
                    .filter(|candidate| *candidate == term)
                    .count();
                if frequency == 0 {
                    continue;
                }
                matched_terms += 1;
                let frequency = frequency as f64;
                let document_frequency = self
                    .search_index
                    .document_frequency
                    .get(term)
                    .copied()
                    .unwrap_or(0) as f64;
                let documents = self.entries.len() as f64;
                let inverse_document_frequency = (1.0
                    + (documents - document_frequency + 0.5) / (document_frequency + 0.5))
                    .ln();
                let length_adjustment = 1.0 - BM25_LENGTH_NORMALIZATION
                    + BM25_LENGTH_NORMALIZATION
                        * (document.len() as f64 / self.search_index.average_length.max(1.0));
                raw_score += inverse_document_frequency * (frequency * (BM25_K1 + 1.0))
                    / (frequency + BM25_K1 * length_adjustment);
            }
            if match_kind.is_none() && raw_score <= 0.0 {
                continue;
            }
            let coverage = if query_terms.is_empty() {
                0
            } else {
                ((matched_terms * 1000) / query_terms.len()) as u16
            };
            let match_kind = match_kind.unwrap_or(SkillMatchKind::Related);
            ranked.push(RankedSuggestion {
                suggestion: SkillSuggestion {
                    id: skill.id.clone(),
                    description: skill.description.clone(),
                    aliases: skill.aliases.clone(),
                    origin: skill.origin,
                    active: self.is_active(&skill.id),
                    match_kind,
                    matched_alias,
                    relevance: match match_kind {
                        SkillMatchKind::ExactId => 1000,
                        SkillMatchKind::ExactAlias => 950,
                        SkillMatchKind::Related => 1,
                    },
                },
                raw_score,
                coverage,
            });
        }

        let maximum_related_score = ranked
            .iter()
            .filter(|item| item.suggestion.match_kind == SkillMatchKind::Related)
            .map(|item| item.raw_score)
            .fold(0.0_f64, f64::max);
        for item in &mut ranked {
            if item.suggestion.match_kind == SkillMatchKind::Related {
                item.suggestion.relevance = if maximum_related_score <= 0.0 {
                    1
                } else {
                    ((item.raw_score / maximum_related_score) * 899.0)
                        .round()
                        .clamp(1.0, 899.0) as u16
                };
            }
        }
        ranked.sort_by(|left, right| {
            match_priority(right.suggestion.match_kind)
                .cmp(&match_priority(left.suggestion.match_kind))
                .then_with(|| {
                    right
                        .raw_score
                        .partial_cmp(&left.raw_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| right.coverage.cmp(&left.coverage))
                .then_with(|| left.suggestion.id.cmp(&right.suggestion.id))
        });
        ranked
            .into_iter()
            .take(limit)
            .map(|item| item.suggestion)
            .collect()
    }

    fn rebuild_search_index(&mut self) {
        let documents = self
            .entries
            .iter()
            .map(|(id, skill)| (id.clone(), weighted_skill_terms(skill)))
            .collect::<BTreeMap<_, _>>();
        let average_length = if documents.is_empty() {
            1.0
        } else {
            documents.values().map(Vec::len).sum::<usize>() as f64 / documents.len() as f64
        };
        let mut document_frequency = HashMap::<String, usize>::new();
        for document in documents.values() {
            for term in document.iter().collect::<HashSet<_>>() {
                *document_frequency.entry(term.clone()).or_default() += 1;
            }
        }
        self.search_index = SkillSearchIndex {
            documents,
            document_frequency,
            average_length,
        };
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.active.iter().any(|skill| skill.reference.id == id)
    }

    pub fn activate_exact(&mut self, id: &str) -> Result<(), SkillError> {
        let selector = id.trim();
        if !self.instances.is_empty() {
            let instance = self.resolve_instance(selector, true)?.clone();
            if self.is_active(&instance.qualified_name) {
                return Ok(());
            }
            let source = self
                .instance_sources
                .get(&instance.instance_id)
                .ok_or_else(|| SkillError::Changed(instance.instance_id.clone()))?;
            let refreshed = load_portable_skill_tree(
                &source.skill_root,
                instance.source_scope,
                &instance.locator,
                instance.enabled,
            )?;
            if refreshed.instance.instance_id != instance.instance_id {
                return Err(SkillError::Changed(instance.instance_id));
            }
            let estimated_tokens = estimate_tokens(&refreshed.body);
            let current_tokens: usize =
                self.active.iter().map(|skill| skill.estimated_tokens).sum();
            if current_tokens.saturating_add(estimated_tokens) > ACTIVE_SKILL_TOKEN_BUDGET {
                return Err(SkillError::BudgetExceeded);
            }
            self.active.push(ActiveSkill {
                reference: ActiveSkillRef {
                    id: instance.qualified_name,
                    path: source.skill_root.join("SKILL.md"),
                    sha256: source.skill_md_sha256.clone(),
                },
                body: refreshed.body,
                estimated_tokens,
            });
            return Ok(());
        }
        let id = selector.to_ascii_lowercase();
        if self.disabled.contains(&id) {
            return Err(SkillError::Disabled(id));
        }
        if self.is_active(&id) {
            return Ok(());
        }
        let metadata = self
            .entries
            .get(&id)
            .cloned()
            .ok_or_else(|| SkillError::Unknown(id.clone()))?;
        let (parsed, sha256) = load_skill(&metadata.path)?;
        if sha256 != metadata.sha256 || parsed.id != metadata.id {
            return Err(SkillError::Changed(id));
        }
        let estimated_tokens = estimate_tokens(&parsed.body);
        let current_tokens: usize = self.active.iter().map(|skill| skill.estimated_tokens).sum();
        if current_tokens.saturating_add(estimated_tokens) > ACTIVE_SKILL_TOKEN_BUDGET {
            return Err(SkillError::BudgetExceeded);
        }
        self.active.push(ActiveSkill {
            reference: ActiveSkillRef {
                id: metadata.id,
                path: metadata.path,
                sha256,
            },
            body: parsed.body,
            estimated_tokens,
        });
        Ok(())
    }

    pub fn deactivate(&mut self, id: &str) -> bool {
        let selector = id.trim();
        let legacy = selector.to_ascii_lowercase();
        let before = self.active.len();
        self.active
            .retain(|skill| skill.reference.id != selector && skill.reference.id != legacy);
        self.active.len() != before
    }

    pub fn view_exact(&self, id: &str) -> Result<String, SkillError> {
        let selector = id.trim();
        if !self.instances.is_empty() {
            let instance = self.resolve_instance(selector, true)?;
            return self.skill_view(&instance.instance_id, "SKILL.md");
        }
        let id = selector.to_ascii_lowercase();
        let metadata = self
            .entries
            .get(&id)
            .ok_or_else(|| SkillError::Unknown(id.clone()))?;
        let (parsed, sha256) = load_skill(&metadata.path)?;
        if sha256 != metadata.sha256 || parsed.id != metadata.id {
            return Err(SkillError::Changed(id));
        }
        Ok(parsed.body)
    }

    pub fn reload(&mut self) {
        let references = self.active_refs();
        let disabled = self.disabled.iter().cloned().collect::<Vec<_>>();
        let mut refreshed = if let Some(roots) = self.portable_roots.clone() {
            Self::discover_portable(roots, &disabled)
        } else {
            Self::discover(self.roots.clone(), &disabled)
        };
        let warnings = refreshed.restore(&references);
        refreshed.diagnostics.extend(warnings);
        *self = refreshed;
    }

    pub fn rediscover_project(&mut self, project: Option<PathBuf>) {
        if let Some(mut roots) = self.portable_roots.clone() {
            roots.retain(|root| {
                root.source_scope != SkillSourceScope::Workspace
                    && !(root.source_scope == SkillSourceScope::Legacy
                        && root.locator == "workspace-ca")
            });
            if let Some(project) = project {
                roots.push(SkillDiscoveryRoot::new(
                    SkillSourceScope::Legacy,
                    project,
                    "workspace-ca",
                ));
            }
            *self =
                Self::discover_portable(roots, &self.disabled.iter().cloned().collect::<Vec<_>>());
            return;
        }
        let mut roots = self.roots.clone();
        roots.project = project;
        roots.project_boundary = roots
            .project
            .as_ref()
            .and_then(|path| path.ancestors().nth(2))
            .map(Path::to_path_buf);
        *self = Self::discover(roots, &self.disabled.iter().cloned().collect::<Vec<_>>());
    }

    pub fn rediscover_workspace(&mut self, workspace: &crate::workspace::WorkspaceContext) {
        let Some(mut roots) = self.portable_roots.clone() else {
            self.rediscover_project(workspace.primary().map(|root| root.path.join(".ca/skills")));
            return;
        };
        roots.retain(|root| {
            root.source_scope != SkillSourceScope::Workspace
                && !(root.source_scope == SkillSourceScope::Legacy
                    && (root.locator == "workspace-ca"
                        || root.locator.starts_with("workspace-ca-")))
        });
        roots.extend(
            workspace
                .skill_ancestor_roots()
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    SkillDiscoveryRoot::new(
                        SkillSourceScope::Workspace,
                        path,
                        format!("ancestor-{index}"),
                    )
                }),
        );
        roots.extend(workspace.roots().iter().enumerate().map(|(index, root)| {
            SkillDiscoveryRoot::new(
                SkillSourceScope::Legacy,
                root.path.join(".ca/skills"),
                format!("workspace-ca-{index}"),
            )
        }));
        *self = Self::discover_portable(roots, &self.disabled.iter().cloned().collect::<Vec<_>>());
    }

    pub fn restore(&mut self, references: &[ActiveSkillRef]) -> Vec<String> {
        let mut warnings = Vec::new();
        for reference in references {
            if !self.instances.is_empty() {
                let portable = self.instance_sources.iter().find_map(|(id, source)| {
                    (source.skill_root.join("SKILL.md") == reference.path
                        && source.skill_md_sha256 == reference.sha256)
                        .then_some(id.clone())
                });
                if let Some(instance_id) = portable {
                    if let Err(error) = self.activate_exact(&instance_id) {
                        warnings.push(error.to_string());
                    }
                    continue;
                }
            }
            let Some(metadata) = self.entries.get(&reference.id) else {
                warnings.push(format!(
                    "skill {} is missing and was not activated",
                    reference.id
                ));
                continue;
            };
            if metadata.path != reference.path || metadata.sha256 != reference.sha256 {
                warnings.push(format!(
                    "skill {} changed and requires reconfirmation",
                    reference.id
                ));
                continue;
            }
            if let Err(error) = self.activate_exact(&reference.id) {
                warnings.push(error.to_string());
            }
        }
        warnings
    }

    pub fn active_refs(&self) -> Vec<ActiveSkillRef> {
        self.active
            .iter()
            .map(|skill| skill.reference.clone())
            .collect()
    }

    pub fn catalog_prompt(&self) -> String {
        let mut output = String::from(
            "## Available skills (metadata only; instructions are not loaded)\n\
             When a skill is relevant to the current request, activate the exact relevant skill before proceeding. Activation is session-scoped and grants no tool authority.\n",
        );
        for skill in self.entries.values().take(MAX_CATALOG_ENTRIES) {
            let aliases = if skill.aliases.is_empty() {
                String::new()
            } else {
                format!("; aliases: {}", skill.aliases.join(", "))
            };
            let line = format!("- {}: {}{}\n", skill.id, skill.description, aliases);
            if output.len().saturating_add(line.len()) > MAX_CATALOG_CHARS {
                break;
            }
            output.push_str(&line);
        }
        output
    }

    pub fn active_prompt(&self) -> String {
        let mut output = String::new();
        for skill in &self.active {
            output.push_str(&format!(
                "\n## Active skill: {}\n<skill-instructions id=\"{}\">\n{}\n</skill-instructions>\n",
                skill.reference.id, skill.reference.id, skill.body
            ));
        }
        output
    }

    pub fn active_prompt_for(&self, id: &str) -> Option<String> {
        self.active
            .iter()
            .find(|skill| skill.reference.id == id)
            .map(format_active_skill)
    }
}

fn runtime_binary_available(value: &str) -> bool {
    let path = Path::new(value);
    if path.components().count() > 1 {
        return fs::metadata(path).is_ok_and(|metadata| metadata.is_file());
    }
    crate::run_environment::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(value))
            .any(|candidate| fs::metadata(candidate).is_ok_and(|metadata| metadata.is_file()))
    })
}

fn format_active_skill(skill: &ActiveSkill) -> String {
    format!(
        "\n## Active skill: {}\n<skill-instructions id=\"{}\">\n{}\n</skill-instructions>\n",
        skill.reference.id, skill.reference.id, skill.body
    )
}

fn identities(metadata: &SkillMetadata) -> Vec<String> {
    std::iter::once(metadata.id.clone())
        .chain(metadata.aliases.iter().cloned())
        .collect()
}

fn match_priority(kind: SkillMatchKind) -> u8 {
    match kind {
        SkillMatchKind::ExactId => 3,
        SkillMatchKind::ExactAlias => 2,
        SkillMatchKind::Related => 1,
    }
}

fn mentions_identity(query: &str, identity: &str) -> bool {
    query.match_indices(identity).any(|(start, _)| {
        let before = query[..start].chars().next_back();
        let end = start + identity.len();
        let after = query[end..].chars().next();
        before.is_none_or(identity_boundary) && after.is_none_or(identity_boundary)
    })
}

fn identity_boundary(character: char) -> bool {
    !character.is_alphanumeric() && character != '-' && character != '_'
}

fn weighted_skill_terms(skill: &SkillMetadata) -> Vec<String> {
    let mut terms = Vec::new();
    for term in tokenize(&skill.id) {
        terms.extend(std::iter::repeat_n(term, 4));
    }
    for alias in &skill.aliases {
        for term in tokenize(alias) {
            terms.extend(std::iter::repeat_n(term, 3));
        }
    }
    terms.extend(tokenize(&skill.description));
    terms
}

fn unique_search_terms(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    tokenize(value)
        .into_iter()
        .filter(|term| !is_search_stop_word(term))
        .filter(|term| seen.insert(term.clone()))
        .collect()
}

fn tokenize(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in value.to_lowercase().chars() {
        if character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

fn is_search_stop_word(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "for"
            | "from"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "use"
            | "with"
    )
}

#[derive(Debug)]
struct PortableLoadResult {
    instance: SkillInstance,
    source: SkillInstanceSource,
    body: String,
}

fn collect_portable_instances(
    roots: &[SkillDiscoveryRoot],
    disabled: &HashSet<String>,
    instances: &mut BTreeMap<String, SkillInstance>,
    sources: &mut HashMap<String, SkillInstanceSource>,
    diagnostics: &mut Vec<String>,
) {
    let declared_roots = roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(&root.path).ok())
        .collect::<Vec<_>>();
    for root in roots {
        if !root.path.exists() {
            continue;
        }
        let canonical_root = match std::fs::canonicalize(&root.path) {
            Ok(path) if path.is_dir() => path,
            Ok(_) => {
                diagnostics.push(format!(
                    "{}: skill root is not a directory",
                    root.path.display()
                ));
                continue;
            }
            Err(error) => {
                diagnostics.push(format!("{}: {error}", root.path.display()));
                continue;
            }
        };
        let mut directories = vec![(root.path.clone(), canonical_root.clone(), 0usize)];
        let mut visited = HashSet::new();
        while let Some((logical_directory, actual_directory, depth)) = directories.pop() {
            if depth > MAX_SKILL_TREE_DEPTH || !visited.insert(actual_directory.clone()) {
                continue;
            }
            let entries = match std::fs::read_dir(&actual_directory) {
                Ok(entries) => entries,
                Err(error) => {
                    diagnostics.push(format!("{}: {error}", logical_directory.display()));
                    continue;
                }
            };
            let mut entries = entries.flatten().collect::<Vec<_>>();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries {
                let logical_path = logical_directory.join(entry.file_name());
                let raw_path = entry.path();
                let metadata = match std::fs::symlink_metadata(&raw_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        diagnostics.push(format!("{}: {error}", logical_path.display()));
                        continue;
                    }
                };
                let actual_path = if metadata.file_type().is_symlink() {
                    let target = match std::fs::canonicalize(&raw_path) {
                        Ok(target) => target,
                        Err(error) => {
                            diagnostics.push(format!("{}: {error}", logical_path.display()));
                            continue;
                        }
                    };
                    if !declared_roots
                        .iter()
                        .any(|declared| declared != &canonical_root && target.starts_with(declared))
                    {
                        diagnostics.push(format!(
                            "{}: symlinked skill folder target is outside declared roots",
                            logical_path.display()
                        ));
                        continue;
                    }
                    target
                } else {
                    raw_path
                };
                if !actual_path.is_dir() {
                    continue;
                }
                if actual_path.join("SKILL.md").is_file() {
                    let relative = logical_path
                        .strip_prefix(&root.path)
                        .unwrap_or(&logical_path);
                    let locator = join_locator(&root.locator, relative);
                    match load_portable_skill_tree(&actual_path, root.source_scope, &locator, true)
                    {
                        Ok(mut loaded) => {
                            loaded.instance.enabled =
                                !disabled.contains(&loaded.instance.metadata.name);
                            let id = loaded.instance.instance_id.clone();
                            if instances.insert(id.clone(), loaded.instance).is_some() {
                                diagnostics.push(format!(
                                    "{}: duplicate immutable skill instance",
                                    logical_path.display()
                                ));
                            } else {
                                sources.insert(id, loaded.source);
                            }
                        }
                        Err(error) => {
                            diagnostics.push(format!("{}: {error}", logical_path.display()))
                        }
                    }
                    continue;
                }
                if depth < MAX_SKILL_TREE_DEPTH {
                    directories.push((logical_path, actual_path, depth + 1));
                }
            }
        }
    }
}

fn join_locator(prefix: &str, relative: &Path) -> String {
    let relative = relative
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    match (prefix.trim_matches('/'), relative.as_str()) {
        ("", relative) => relative.to_string(),
        (prefix, "") => prefix.to_string(),
        (prefix, relative) => format!("{prefix}/{relative}"),
    }
}

fn percent_encode_locator(locator: &str) -> String {
    let mut output = String::new();
    for byte in locator.as_bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-' {
            output.push(char::from(*byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn load_portable_skill_tree(
    skill_root: &Path,
    source_scope: SkillSourceScope,
    locator: &str,
    enabled: bool,
) -> Result<PortableLoadResult, SkillError> {
    let canonical_root =
        std::fs::canonicalize(skill_root).map_err(|error| SkillError::Io(error.to_string()))?;
    if !canonical_root.is_dir() {
        return Err(SkillError::Invalid("skill root is not a directory".into()));
    }
    let skill_md = canonical_root.join("SKILL.md");
    let skill_bytes = read_bounded_skill_file(&skill_md, MAX_SKILL_BYTES)?;
    let skill_md_sha256 = format!("{:x}", Sha256::digest(&skill_bytes));
    let skill_text = std::str::from_utf8(&skill_bytes)
        .map_err(|_| SkillError::Invalid("SKILL.md is not valid UTF-8".into()))?;
    let mut parsed = parse_portable_skill(skill_text)?;
    if source_scope != SkillSourceScope::Legacy {
        let directory_name = canonical_root.file_name().and_then(|name| name.to_str());
        if directory_name != Some(parsed.metadata.name.as_str()) {
            return Err(SkillError::Invalid(
                "portable skill name must match its parent directory".into(),
            ));
        }
    }

    let mut files = inventory_skill_tree(&canonical_root)?;
    let openai_path = canonical_root.join("agents/openai.yaml");
    if openai_path.exists() {
        let bytes = read_bounded_skill_file(&openai_path, MAX_SKILL_RESOURCE_BYTES)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| SkillError::Invalid("agents/openai.yaml is not valid UTF-8".into()))?;
        apply_openai_metadata(text, &mut parsed.metadata)?;
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut digest = Sha256::new();
    digest.update(b"ca.skill-content.v1\0");
    for file in &files {
        let bytes = read_skill_resource_bytes(&canonical_root, &file.relative_path)?;
        let path = file.relative_path.to_string_lossy();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update(file.mode.to_be_bytes());
        digest.update(file.size.to_be_bytes());
        digest.update(&bytes);
    }
    let content_digest = format!("{:x}", digest.finalize());
    let qualified_name = format!(
        "{}::{}::{}",
        source_scope.label(),
        percent_encode_locator(locator),
        parsed.metadata.name
    );
    let mut id_digest = Sha256::new();
    id_digest.update(b"ca.skill-instance.v1\0");
    id_digest.update(qualified_name.as_bytes());
    id_digest.update([0]);
    id_digest.update(content_digest.as_bytes());
    let instance_id = format!("{:x}", id_digest.finalize());
    Ok(PortableLoadResult {
        instance: SkillInstance {
            instance_id,
            qualified_name,
            source_scope,
            locator: locator.to_string(),
            content_digest,
            metadata: parsed.metadata,
            files,
            enabled,
            availability: SkillAvailability::default(),
        },
        source: SkillInstanceSource {
            skill_root: canonical_root,
            skill_md_sha256,
        },
        body: parsed.body,
    })
}

pub fn explicit_skill_selectors(input: &str) -> Vec<String> {
    let mut selectors = input
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('$'))
        .map(|word| {
            word.trim_matches(|character: char| {
                matches!(
                    character,
                    ',' | '.' | ':' | ';' | '!' | '?' | ')' | ']' | '}'
                )
            })
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    selectors.sort();
    selectors.dedup();
    selectors
}

pub fn resolve_pinned_instance<'a>(
    instances: impl IntoIterator<Item = &'a SkillInstance>,
    instance_id: &str,
    full_digest: &str,
) -> Result<&'a SkillInstance, SkillError> {
    let instance = instances
        .into_iter()
        .find(|instance| instance.instance_id == instance_id)
        .ok_or_else(|| SkillError::Unknown(instance_id.to_string()))?;
    if instance.content_digest != full_digest {
        return Err(SkillError::Changed(instance_id.to_string()));
    }
    Ok(instance)
}

pub(crate) fn portable_skill_tree_digest(
    root: &Path,
    source_scope: SkillSourceScope,
) -> Result<String, SkillError> {
    load_portable_skill_tree(root, source_scope, "curator-verification", true)
        .map(|loaded| loaded.instance.content_digest)
}

fn ensure_writable_skill_root(root: &Path) -> Result<(), SkillError> {
    fs::create_dir_all(root).map_err(|error| SkillError::Io(error.to_string()))?;
    let metadata = fs::symlink_metadata(root).map_err(|error| SkillError::Io(error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SkillError::Invalid(
            "writable skill root must be a real directory".into(),
        ));
    }
    Ok(())
}

fn managed_skill_relative_path(value: &str) -> Result<PathBuf, SkillError> {
    let relative = validate_skill_relative_path(value)?;
    let mut components = relative.components();
    let first = components
        .next()
        .and_then(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .ok_or_else(|| SkillError::Invalid("invalid skill resource path".into()))?;
    let allowed = match first {
        "SKILL.md" => components.next().is_none(),
        "references" | "templates" | "scripts" | "assets" => true,
        "agents" => relative == Path::new("agents/openai.yaml"),
        _ => false,
    };
    if !allowed {
        return Err(SkillError::Invalid(
            "skill resources must be SKILL.md or live under references/, templates/, scripts/, assets/, or agents/openai.yaml"
                .into(),
        ));
    }
    Ok(relative)
}

pub(crate) fn validate_generated_resource_path(value: &str) -> Result<PathBuf, SkillError> {
    let relative = managed_skill_relative_path(value)?;
    if relative == Path::new("SKILL.md") {
        return Err(SkillError::Invalid(
            "generated resources cannot replace SKILL.md".into(),
        ));
    }
    Ok(relative)
}

fn write_text_skill_file(root: &Path, relative: &Path, content: &str) -> Result<(), SkillError> {
    if content.as_bytes().contains(&0)
        || content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(SkillError::Invalid(
            "skill resource contains binary or unsafe control content".into(),
        ));
    }
    let limit = if relative == Path::new("SKILL.md") {
        MAX_SKILL_BYTES
    } else {
        MAX_SKILL_RESOURCE_BYTES
    };
    if content.len() as u64 > limit {
        return Err(SkillError::Invalid(format!(
            "skill resource exceeds byte limit: {}",
            relative.display()
        )));
    }
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| SkillError::Io(error.to_string()))?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&target)
        .map_err(|error| SkillError::Io(error.to_string()))?;
    file.write_all(content.as_bytes())
        .map_err(|error| SkillError::Io(error.to_string()))?;
    file.sync_all()
        .map_err(|error| SkillError::Io(error.to_string()))
}

fn copy_skill_tree_text(source: &Path, target: &Path) -> Result<(), SkillError> {
    for file in inventory_skill_tree(source)? {
        let text = read_skill_resource(source, &file.relative_path)?;
        write_text_skill_file(target, &file.relative_path, &text)?;
    }
    Ok(())
}

fn apply_skill_tree_request(root: &Path, request: &SkillManageRequest) -> Result<(), SkillError> {
    match request.action {
        SkillManageAction::Create | SkillManageAction::Patch => {
            if request.files.is_empty()
                || request.relative_path.is_some()
                || request.content.is_some()
                || request.old_text.is_some()
                || request.new_text.is_some()
            {
                return Err(SkillError::Invalid(
                    "create/patch requires only a non-empty files list".into(),
                ));
            }
            let mut seen = HashSet::new();
            for file in &request.files {
                let relative = managed_skill_relative_path(&file.path)?;
                if !seen.insert(relative.clone()) {
                    return Err(SkillError::Invalid(
                        "skill mutation repeats a resource path".into(),
                    ));
                }
                match &file.content {
                    Some(content) => write_text_skill_file(root, &relative, content)?,
                    None if request.action == SkillManageAction::Patch => {
                        let target = root.join(&relative);
                        let metadata = fs::symlink_metadata(&target)
                            .map_err(|_| SkillError::Changed(file.path.clone()))?;
                        if !metadata.is_file() || metadata.file_type().is_symlink() {
                            return Err(SkillError::Invalid(
                                "skill resource removal target is not a regular file".into(),
                            ));
                        }
                        fs::remove_file(target)
                            .map_err(|error| SkillError::Io(error.to_string()))?;
                    }
                    None => {
                        return Err(SkillError::Invalid(
                            "create file content cannot be null".into(),
                        ))
                    }
                }
            }
        }
        SkillManageAction::WriteFile => {
            if !request.files.is_empty() || request.old_text.is_some() || request.new_text.is_some()
            {
                return Err(SkillError::Invalid("write_file shape is invalid".into()));
            }
            let relative =
                managed_skill_relative_path(request.relative_path.as_deref().ok_or_else(
                    || SkillError::Invalid("write_file requires relative_path".into()),
                )?)?;
            write_text_skill_file(
                root,
                &relative,
                request
                    .content
                    .as_deref()
                    .ok_or_else(|| SkillError::Invalid("write_file requires content".into()))?,
            )?;
        }
        SkillManageAction::RemoveFile => {
            if !request.files.is_empty()
                || request.content.is_some()
                || request.old_text.is_some()
                || request.new_text.is_some()
            {
                return Err(SkillError::Invalid("remove_file shape is invalid".into()));
            }
            let relative =
                managed_skill_relative_path(request.relative_path.as_deref().ok_or_else(
                    || SkillError::Invalid("remove_file requires relative_path".into()),
                )?)?;
            if relative == Path::new("SKILL.md") {
                return Err(SkillError::Invalid("SKILL.md cannot be removed".into()));
            }
            let target = root.join(relative);
            let metadata = fs::symlink_metadata(&target)
                .map_err(|_| SkillError::Changed(target.display().to_string()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(SkillError::Invalid(
                    "skill resource removal target is not a regular file".into(),
                ));
            }
            fs::remove_file(target).map_err(|error| SkillError::Io(error.to_string()))?;
        }
        SkillManageAction::Edit => {
            if !request.files.is_empty() || request.content.is_some() {
                return Err(SkillError::Invalid("edit shape is invalid".into()));
            }
            let relative = managed_skill_relative_path(
                request
                    .relative_path
                    .as_deref()
                    .ok_or_else(|| SkillError::Invalid("edit requires relative_path".into()))?,
            )?;
            let old = request
                .old_text
                .as_deref()
                .filter(|text| !text.is_empty())
                .ok_or_else(|| SkillError::Invalid("edit requires non-empty old_text".into()))?;
            let new = request
                .new_text
                .as_deref()
                .ok_or_else(|| SkillError::Invalid("edit requires new_text".into()))?;
            let current = read_skill_resource(root, &relative)?;
            if current.matches(old).count() != 1 {
                return Err(SkillError::Changed(relative.display().to_string()));
            }
            write_text_skill_file(root, &relative, &current.replacen(old, new, 1))?;
        }
        SkillManageAction::Delete => unreachable!("delete is handled before staging"),
    }
    Ok(())
}

fn validate_staged_skill_tree(
    root: &Path,
    _source_scope: SkillSourceScope,
    expected_name: &str,
) -> Result<(), SkillError> {
    let loaded = load_portable_skill_tree(root, SkillSourceScope::Legacy, "staged", true)?;
    if loaded.instance.metadata.name != expected_name {
        return Err(SkillError::Invalid(
            "mutating a skill cannot rename its exact selector".into(),
        ));
    }
    if crate::tui::commands::find_command(expected_name).is_some() {
        return Err(SkillError::Invalid(
            "skill name collides with a built-in command".into(),
        ));
    }
    Ok(())
}

fn skill_journal_root(discovery_root: &Path) -> Result<PathBuf, SkillError> {
    let parent = discovery_root
        .parent()
        .ok_or_else(|| SkillError::Invalid("skill discovery root has no parent".into()))?;
    let root = parent.join(".ca-skill-journal");
    ensure_writable_skill_root(&root)?;
    Ok(root)
}

fn skill_journal_backup(
    discovery_root: &Path,
    journal_id: uuid::Uuid,
    name: &str,
) -> Result<PathBuf, SkillError> {
    let root = skill_journal_root(discovery_root)?.join(journal_id.to_string());
    fs::create_dir(&root).map_err(|error| SkillError::Io(error.to_string()))?;
    Ok(root.join(format!("{name}.before")))
}

#[allow(clippy::too_many_arguments)]
fn write_skill_journal(
    discovery_root: &Path,
    journal_id: uuid::Uuid,
    phase: &str,
    action: SkillManageAction,
    target: &Path,
    backup: Option<&Path>,
    before_digest: &str,
    after_digest: Option<&str>,
) -> Result<(), SkillError> {
    let root = skill_journal_root(discovery_root)?.join(journal_id.to_string());
    fs::create_dir_all(&root).map_err(|error| SkillError::Io(error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1,
        "journal_id": journal_id,
        "phase": phase,
        "action": action,
        "target": target,
        "backup": backup,
        "before_digest": before_digest,
        "after_digest": after_digest,
    }))
    .map_err(|error| SkillError::Io(error.to_string()))?;
    let path = root.join("journal.json");
    let temp = root.join("journal.json.tmp");
    fs::write(&temp, bytes).map_err(|error| SkillError::Io(error.to_string()))?;
    fs::rename(temp, path).map_err(|error| SkillError::Io(error.to_string()))
}

fn recover_portable_skill_journals(roots: &[SkillDiscoveryRoot], diagnostics: &mut Vec<String>) {
    let mut visited = HashSet::new();
    for discovery in roots {
        let Some(parent) = discovery.path.parent() else {
            continue;
        };
        let journal_root = parent.join(".ca-skill-journal");
        if !journal_root.is_dir() || !visited.insert(journal_root.clone()) {
            continue;
        }
        let entries = match fs::read_dir(&journal_root) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(error) => {
                diagnostics.push(format!("skill journal recovery: {error}"));
                continue;
            }
        };
        for entry in entries {
            let journal_path = entry.path().join("journal.json");
            let result = (|| -> Result<(), SkillError> {
                let metadata = fs::symlink_metadata(&journal_path)
                    .map_err(|error| SkillError::Io(error.to_string()))?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > 64 * 1024
                {
                    return Err(SkillError::Invalid(
                        "skill journal is not a bounded regular file".into(),
                    ));
                }
                let mut value: Value = serde_json::from_slice(
                    &fs::read(&journal_path).map_err(|error| SkillError::Io(error.to_string()))?,
                )
                .map_err(|error| SkillError::Invalid(error.to_string()))?;
                if value.get("phase").and_then(Value::as_str) != Some("prepared") {
                    return Ok(());
                }
                let target = value
                    .get("target")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .ok_or_else(|| SkillError::Invalid("skill journal target is invalid".into()))?;
                let canonical_discovery = fs::canonicalize(&discovery.path).ok();
                if !target.starts_with(&discovery.path)
                    && !canonical_discovery
                        .as_ref()
                        .is_some_and(|root| target.starts_with(root))
                {
                    return Ok(());
                }
                let backup = value
                    .get("backup")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                if backup
                    .as_ref()
                    .is_some_and(|backup| !backup.starts_with(&journal_root))
                {
                    return Err(SkillError::Invalid(
                        "skill journal backup escaped its root".into(),
                    ));
                }
                if let Some(backup) = backup.filter(|backup| backup.exists()) {
                    if target.exists() {
                        fs::remove_dir_all(&target)
                            .map_err(|error| SkillError::Io(error.to_string()))?;
                    }
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|error| SkillError::Io(error.to_string()))?;
                    }
                    fs::rename(backup, &target)
                        .map_err(|error| SkillError::Io(error.to_string()))?;
                } else if value.get("action").and_then(Value::as_str) == Some("create")
                    && target.exists()
                {
                    fs::remove_dir_all(&target)
                        .map_err(|error| SkillError::Io(error.to_string()))?;
                }
                if let Some(object) = value.as_object_mut() {
                    object.insert("phase".into(), Value::String("rolled_back".into()));
                }
                let bytes = serde_json::to_vec_pretty(&value)
                    .map_err(|error| SkillError::Io(error.to_string()))?;
                fs::write(&journal_path, bytes)
                    .map_err(|error| SkillError::Io(error.to_string()))?;
                Ok(())
            })();
            if let Err(error) = result {
                diagnostics.push(format!("{}: {error}", journal_path.display()));
            }
        }
    }
}

fn inventory_skill_tree(skill_root: &Path) -> Result<Vec<SkillFile>, SkillError> {
    let mut paths = vec![PathBuf::from("SKILL.md")];
    for directory in ["references", "templates", "scripts", "assets"] {
        collect_inventory_paths(skill_root, Path::new(directory), 1, &mut paths)?;
    }
    if skill_root.join("agents/openai.yaml").exists() {
        paths.push(PathBuf::from("agents/openai.yaml"));
    }
    paths.sort();
    paths.dedup();
    if paths.len() > MAX_SKILL_TREE_FILES {
        return Err(SkillError::Invalid(format!(
            "skill tree exceeds {MAX_SKILL_TREE_FILES} files"
        )));
    }
    let mut aggregate = 0u64;
    let mut files = Vec::with_capacity(paths.len());
    for relative_path in paths {
        let path = confined_skill_path(skill_root, &relative_path)?;
        let metadata =
            std::fs::metadata(&path).map_err(|error| SkillError::Io(error.to_string()))?;
        if !metadata.is_file() {
            return Err(SkillError::Invalid(format!(
                "skill resource is not a regular file: {}",
                relative_path.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(SkillError::Invalid(format!(
                    "skill resource is hard-linked: {}",
                    relative_path.display()
                )));
            }
        }
        let limit = if relative_path == Path::new("SKILL.md") {
            MAX_SKILL_BYTES
        } else {
            MAX_SKILL_RESOURCE_BYTES
        };
        if metadata.len() > limit {
            return Err(SkillError::Invalid(format!(
                "skill resource exceeds byte limit: {}",
                relative_path.display()
            )));
        }
        aggregate = aggregate
            .checked_add(metadata.len())
            .ok_or_else(|| SkillError::Invalid("skill aggregate size overflow".into()))?;
        if aggregate > MAX_SKILL_TREE_BYTES {
            return Err(SkillError::Invalid(format!(
                "skill tree exceeds {MAX_SKILL_TREE_BYTES} bytes"
            )));
        }
        let bytes = read_bounded_skill_file(&path, limit)?;
        std::str::from_utf8(&bytes).map_err(|_| {
            SkillError::Invalid(format!(
                "skill resource is binary: {}",
                relative_path.display()
            ))
        })?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        };
        #[cfg(not(unix))]
        let mode = 0;
        files.push(SkillFile {
            relative_path,
            size: metadata.len(),
            mode,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
    }
    Ok(files)
}

fn collect_inventory_paths(
    skill_root: &Path,
    relative: &Path,
    depth: usize,
    output: &mut Vec<PathBuf>,
) -> Result<(), SkillError> {
    if depth > MAX_SKILL_TREE_DEPTH {
        return Err(SkillError::Invalid(format!(
            "skill tree exceeds depth {MAX_SKILL_TREE_DEPTH}"
        )));
    }
    let path = skill_root.join(relative);
    if !path.exists() {
        return Ok(());
    }
    let confined = confined_skill_path(skill_root, relative)?;
    let metadata =
        std::fs::metadata(&confined).map_err(|error| SkillError::Io(error.to_string()))?;
    if metadata.is_file() {
        output.push(relative.to_path_buf());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(SkillError::Invalid(format!(
            "skill resource is not regular: {}",
            relative.display()
        )));
    }
    let mut entries = std::fs::read_dir(&confined)
        .map_err(|error| SkillError::Io(error.to_string()))?
        .flatten()
        .collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let child = relative.join(entry.file_name());
        collect_inventory_paths(skill_root, &child, depth + 1, output)?;
        if output.len() > MAX_SKILL_TREE_FILES {
            return Err(SkillError::Invalid(format!(
                "skill tree exceeds {MAX_SKILL_TREE_FILES} files"
            )));
        }
    }
    Ok(())
}

fn confined_skill_path(skill_root: &Path, relative: &Path) -> Result<PathBuf, SkillError> {
    let relative = validate_skill_path(relative)?;
    let path = skill_root.join(&relative);
    let canonical =
        std::fs::canonicalize(&path).map_err(|error| SkillError::Io(error.to_string()))?;
    if !canonical.starts_with(skill_root) {
        return Err(SkillError::Invalid(format!(
            "skill resource escapes its root: {}",
            relative.display()
        )));
    }
    Ok(canonical)
}

fn validate_skill_relative_path(value: &str) -> Result<PathBuf, SkillError> {
    validate_skill_path(Path::new(value))
}

fn validate_skill_path(value: &Path) -> Result<PathBuf, SkillError> {
    if value.as_os_str().is_empty() || value.is_absolute() {
        return Err(SkillError::Invalid(
            "skill resource path must be relative".into(),
        ));
    }
    let mut normalized = PathBuf::new();
    for component in value.components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            _ => {
                return Err(SkillError::Invalid(
                    "skill resource path contains traversal".into(),
                ))
            }
        }
    }
    Ok(normalized)
}

fn read_bounded_skill_file(path: &Path, limit: u64) -> Result<Vec<u8>, SkillError> {
    let metadata = std::fs::metadata(path).map_err(|error| SkillError::Io(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(SkillError::Invalid(format!(
            "invalid or oversized skill file: {}",
            path.display()
        )));
    }
    let bytes = std::fs::read(path).map_err(|error| SkillError::Io(error.to_string()))?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > limit {
        return Err(SkillError::Changed(path.display().to_string()));
    }
    Ok(bytes)
}

fn read_skill_resource_bytes(skill_root: &Path, relative: &Path) -> Result<Vec<u8>, SkillError> {
    let path = confined_skill_path(skill_root, relative)?;
    let limit = if relative == Path::new("SKILL.md") {
        MAX_SKILL_BYTES
    } else {
        MAX_SKILL_RESOURCE_BYTES
    };
    read_bounded_skill_file(&path, limit)
}

fn read_skill_resource(skill_root: &Path, relative: &Path) -> Result<String, SkillError> {
    let bytes = read_skill_resource_bytes(skill_root, relative)?;
    String::from_utf8(bytes)
        .map_err(|_| SkillError::Invalid("skill resource is not valid UTF-8".into()))
}

fn parse_portable_skill(text: &str) -> Result<PortableParsedSkill, SkillError> {
    let normalized = text.replace("\r\n", "\n");
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return Err(SkillError::Invalid(
            "missing opening frontmatter delimiter".into(),
        ));
    };
    let Some((frontmatter, body)) = rest.split_once("\n---\n") else {
        return Err(SkillError::Invalid(
            "missing closing frontmatter delimiter".into(),
        ));
    };
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(frontmatter)
        .map_err(|error| SkillError::Invalid(format!("invalid YAML frontmatter: {error}")))?;
    let mapping = yaml_mapping(&value, "skill frontmatter")?;
    let allowed = [
        "name",
        "description",
        "category",
        "related_skills",
        "aliases",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
        // Common Hermes/Agent-Skills descriptive metadata. These fields are
        // intentionally ignored: they cannot grant tools, install declared
        // dependencies, select a model, or change runtime authority.
        "version",
        "author",
        "platforms",
        "prerequisites",
        "dependencies",
        "tags",
        "triggers",
        "title",
        "setup",
        "required_credential_files",
        "required_environment_variables",
        "environments",
    ];
    reject_unknown_yaml_keys(mapping, &allowed, "frontmatter")?;

    let name = yaml_required_string(mapping, "name", 64)?;
    validate_portable_name(&name)?;
    let description = yaml_required_string(mapping, "description", MAX_DESCRIPTION_CHARS)?;
    let (category, related_skills) = parse_skill_relationship_metadata(mapping, &name)?;
    let license = yaml_optional_string(mapping, "license", 500)?;
    let compatibility = yaml_optional_string(mapping, "compatibility", 500)?;
    let allowed_tools = yaml_optional_string(mapping, "allowed-tools", 2_048)?;
    let aliases = yaml_string_list(mapping, "aliases", 16, 64)?;
    let conditions = parse_availability_conditions(mapping)?;
    let mut identities = HashSet::from([name.clone()]);
    for alias in &aliases {
        validate_identity(alias)?;
        if !identities.insert(alias.clone()) {
            return Err(SkillError::Invalid(format!("duplicate identity: {alias}")));
        }
    }
    let metadata_keys = match yaml_value(mapping, "metadata") {
        None => Vec::new(),
        Some(value) => {
            let values = yaml_mapping(value, "metadata")?;
            if values.len() > 64 {
                return Err(SkillError::Invalid(
                    "metadata contains more than 64 entries".into(),
                ));
            }
            let mut keys = values
                .keys()
                .map(|key| {
                    let key = key.as_str().ok_or_else(|| {
                        SkillError::Invalid("metadata keys must be strings".into())
                    })?;
                    if key.is_empty() || key.chars().count() > 128 {
                        return Err(SkillError::Invalid(
                            "metadata keys must contain 1 to 128 characters".into(),
                        ));
                    }
                    Ok(key.to_string())
                })
                .collect::<Result<Vec<_>, SkillError>>()?;
            keys.sort();
            keys
        }
    };
    let body = body.trim().to_string();
    if body.is_empty() {
        return Err(SkillError::Invalid("skill instructions are empty".into()));
    }
    Ok(PortableParsedSkill {
        metadata: SkillInstanceMetadata {
            name,
            description,
            category,
            related_skills,
            aliases,
            license,
            compatibility,
            metadata_keys,
            allowed_tools,
            allow_implicit_invocation: true,
            dependencies: Vec::new(),
            conditions,
        },
        body,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoDistilledSkillValidation {
    pub name: String,
    pub description: String,
    pub sha256: String,
    pub full_tree_digest: String,
}

pub fn validate_auto_distilled_skill(
    text: &str,
    run_id: uuid::Uuid,
) -> Result<AutoDistilledSkillValidation, SkillError> {
    validate_auto_distilled_skill_tree(text, &[], run_id)
}

pub fn validate_auto_distilled_skill_tree(
    text: &str,
    resources: &[(String, String)],
    run_id: uuid::Uuid,
) -> Result<AutoDistilledSkillValidation, SkillError> {
    if text.is_empty() || text.len() as u64 > MAX_SKILL_BYTES {
        return Err(SkillError::Invalid(
            "auto-distilled SKILL.md exceeds its byte bound".into(),
        ));
    }
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(SkillError::Invalid(
            "auto-distilled SKILL.md contains unsafe controls".into(),
        ));
    }
    let parsed = parse_portable_skill(text)?;
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| SkillError::Invalid("missing opening frontmatter delimiter".into()))?;
    let (frontmatter, body) = rest
        .split_once("\n---\n")
        .ok_or_else(|| SkillError::Invalid("missing closing frontmatter delimiter".into()))?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(frontmatter)
        .map_err(|error| SkillError::Invalid(format!("invalid YAML frontmatter: {error}")))?;
    let mapping = yaml_mapping(&value, "skill frontmatter")?;
    let metadata = yaml_value(mapping, "metadata")
        .ok_or_else(|| SkillError::Invalid("auto-distilled provenance metadata is required".into()))
        .and_then(|value| yaml_mapping(value, "metadata"))?;
    let run_id_text = run_id.to_string();
    if metadata.len() != 2
        || metadata
            .get(serde_yaml_ng::Value::String("ca_origin".into()))
            .and_then(serde_yaml_ng::Value::as_str)
            != Some("auto_distill")
        || metadata
            .get(serde_yaml_ng::Value::String("ca_run_id".into()))
            .and_then(serde_yaml_ng::Value::as_str)
            != Some(run_id_text.as_str())
    {
        return Err(SkillError::Invalid(
            "auto-distilled provenance metadata must contain only ca_origin=auto_distill and the exact ca_run_id"
                .into(),
        ));
    }
    let lower = body.to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "plugin.json",
        ".codex-plugin",
        "spawn_agent",
        "subagent",
        "sandbox exception",
        "bypass approval",
        "ignore approval",
        "grant permission",
    ];
    if FORBIDDEN.iter().any(|marker| lower.contains(marker)) {
        return Err(SkillError::Invalid(
            "auto-distilled skill contains forbidden authority-bearing content".into(),
        ));
    }
    let temp = tempfile::tempdir().map_err(|error| SkillError::Io(error.to_string()))?;
    let root = temp.path().join(&parsed.metadata.name);
    fs::create_dir(&root).map_err(|error| SkillError::Io(error.to_string()))?;
    write_text_skill_file(&root, Path::new("SKILL.md"), text)?;
    let mut seen = HashSet::new();
    for (path, content) in resources {
        let relative = managed_skill_relative_path(path)?;
        if relative == Path::new("SKILL.md") || !seen.insert(relative.clone()) {
            return Err(SkillError::Invalid(
                "generated skill repeats SKILL.md or a resource path".into(),
            ));
        }
        write_text_skill_file(&root, &relative, content)?;
    }
    let loaded =
        load_portable_skill_tree(&root, SkillSourceScope::AutoDistill, "validation", true)?;
    Ok(AutoDistilledSkillValidation {
        name: parsed.metadata.name,
        description: parsed.metadata.description,
        sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
        full_tree_digest: loaded.instance.content_digest,
    })
}

fn apply_openai_metadata(
    text: &str,
    metadata: &mut SkillInstanceMetadata,
) -> Result<(), SkillError> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)
        .map_err(|error| SkillError::Invalid(format!("invalid agents/openai.yaml: {error}")))?;
    let mapping = yaml_mapping(&value, "agents/openai.yaml")?;
    reject_unknown_yaml_keys(
        mapping,
        &["interface", "policy", "dependencies"],
        "openai.yaml",
    )?;
    if let Some(policy) = yaml_value(mapping, "policy") {
        let policy = yaml_mapping(policy, "policy")?;
        reject_unknown_yaml_keys(policy, &["allow_implicit_invocation"], "policy")?;
        if let Some(value) = yaml_value(policy, "allow_implicit_invocation") {
            metadata.allow_implicit_invocation = value.as_bool().ok_or_else(|| {
                SkillError::Invalid("allow_implicit_invocation must be a boolean".into())
            })?;
        }
    }
    if let Some(interface) = yaml_value(mapping, "interface") {
        let interface = yaml_mapping(interface, "interface")?;
        if interface.len() > 32 {
            return Err(SkillError::Invalid(
                "interface contains more than 32 entries".into(),
            ));
        }
    }
    if let Some(dependencies) = yaml_value(mapping, "dependencies") {
        let dependencies = yaml_mapping(dependencies, "dependencies")?;
        reject_unknown_yaml_keys(dependencies, &["tools"], "dependencies")?;
        if let Some(tools) = yaml_value(dependencies, "tools") {
            let tools = tools
                .as_sequence()
                .ok_or_else(|| SkillError::Invalid("dependencies.tools must be a list".into()))?;
            if tools.len() > 32 {
                return Err(SkillError::Invalid(
                    "dependencies.tools contains more than 32 entries".into(),
                ));
            }
            metadata.dependencies = tools
                .iter()
                .map(parse_skill_dependency)
                .collect::<Result<Vec<_>, _>>()?;
        }
    }
    Ok(())
}

fn parse_skill_dependency(value: &serde_yaml_ng::Value) -> Result<SkillDependency, SkillError> {
    let mapping = yaml_mapping(value, "dependency")?;
    reject_unknown_yaml_keys(
        mapping,
        &["type", "value", "description", "transport", "url"],
        "dependency",
    )?;
    let url = yaml_optional_string(mapping, "url", 2_048)?;
    if let Some(raw_url) = &url {
        let parsed = url::Url::parse(raw_url)
            .map_err(|_| SkillError::Invalid("dependency URL is invalid".into()))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(SkillError::Invalid(
                "dependency URL must be credential-free HTTP(S) without query or fragment".into(),
            ));
        }
    }
    Ok(SkillDependency {
        dependency_type: yaml_required_string(mapping, "type", 64)?,
        value: yaml_required_string(mapping, "value", 256)?,
        description: yaml_optional_string(mapping, "description", 1_024)?,
        transport: yaml_optional_string(mapping, "transport", 64)?,
        url,
    })
}

fn parse_availability_conditions(
    frontmatter: &serde_yaml_ng::Mapping,
) -> Result<SkillAvailabilityConditions, SkillError> {
    let mut conditions = SkillAvailabilityConditions {
        platforms: yaml_string_list(frontmatter, "platforms", 16, 64)?,
        required_environment_variables: yaml_case_sensitive_string_list(
            frontmatter,
            "required_environment_variables",
            32,
            128,
        )?,
        ..SkillAvailabilityConditions::default()
    };
    for name in &conditions.required_environment_variables {
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(SkillError::Invalid(format!(
                "required environment variable must be an uppercase identifier: {name}"
            )));
        }
    }
    let Some(metadata) = yaml_value(frontmatter, "metadata") else {
        return Ok(conditions);
    };
    let metadata = yaml_mapping(metadata, "metadata")?;
    let Some(hermes) = yaml_value(metadata, "hermes") else {
        return Ok(conditions);
    };
    let hermes = yaml_mapping(hermes, "metadata.hermes")?;
    reject_unknown_yaml_keys(
        hermes,
        &[
            "requires_tools",
            "requires_toolsets",
            "fallback_for_tools",
            "fallback_for_toolsets",
            "config",
            "category",
            "related_skills",
            "required_environment_variables",
        ],
        "metadata.hermes",
    )?;
    conditions.requires_tools = yaml_string_list(hermes, "requires_tools", 32, 128)?;
    conditions.requires_toolsets = yaml_string_list(hermes, "requires_toolsets", 16, 128)?;
    conditions.fallback_for_tools = yaml_string_list(hermes, "fallback_for_tools", 32, 128)?;
    conditions.fallback_for_toolsets = yaml_string_list(hermes, "fallback_for_toolsets", 16, 128)?;
    let nested_environment =
        yaml_case_sensitive_string_list(hermes, "required_environment_variables", 32, 128)?;
    conditions
        .required_environment_variables
        .extend(nested_environment);
    conditions.required_environment_variables.sort();
    conditions.required_environment_variables.dedup();
    for name in &conditions.required_environment_variables {
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(SkillError::Invalid(format!(
                "required environment variable must be an uppercase identifier: {name}"
            )));
        }
    }
    if let Some(config) = yaml_value(hermes, "config") {
        let config = yaml_mapping(config, "metadata.hermes.config")?;
        if config.len() > 32 {
            return Err(SkillError::Invalid(
                "metadata.hermes.config contains more than 32 entries".into(),
            ));
        }
        for (raw_key, raw_declaration) in config {
            let key = raw_key
                .as_str()
                .ok_or_else(|| SkillError::Invalid("skill config keys must be strings".into()))?;
            validate_setting_key(key)?;
            let declaration = yaml_mapping(raw_declaration, "skill config declaration")?;
            reject_unknown_yaml_keys(
                declaration,
                &["description", "default", "required"],
                "skill config declaration",
            )?;
            let description = yaml_optional_string(declaration, "description", 512)?;
            let required = yaml_value(declaration, "required")
                .map(|value| {
                    value.as_bool().ok_or_else(|| {
                        SkillError::Invalid("skill config required must be boolean".into())
                    })
                })
                .transpose()?
                .unwrap_or(true);
            let default = yaml_value(declaration, "default")
                .map(parse_setting_value)
                .transpose()?;
            conditions.config.push(SkillConfigDeclaration {
                key: key.to_string(),
                description,
                default,
                required,
            });
        }
        conditions
            .config
            .sort_by(|left, right| left.key.cmp(&right.key));
    }
    Ok(conditions)
}

fn parse_skill_relationship_metadata(
    frontmatter: &serde_yaml_ng::Mapping,
    skill_name: &str,
) -> Result<(Option<String>, Vec<String>), SkillError> {
    let top_category = yaml_optional_string(frontmatter, "category", 64)?
        .map(|value| normalize_skill_category(&value))
        .transpose()?;
    let mut related = yaml_relationship_list(frontmatter, "related_skills")?;
    let mut nested_category = None;
    if let Some(metadata) = yaml_value(frontmatter, "metadata") {
        let metadata = yaml_mapping(metadata, "metadata")?;
        if let Some(hermes) = yaml_value(metadata, "hermes") {
            let hermes = yaml_mapping(hermes, "metadata.hermes")?;
            nested_category = yaml_optional_string(hermes, "category", 64)?
                .map(|value| normalize_skill_category(&value))
                .transpose()?;
            related.extend(yaml_relationship_list(hermes, "related_skills")?);
        }
    }
    if related.len() > 16 {
        return Err(SkillError::Invalid(
            "related_skills contains more than 16 entries".into(),
        ));
    }
    if let (Some(top), Some(nested)) = (&top_category, &nested_category) {
        if top != nested {
            return Err(SkillError::Invalid(
                "category conflicts with metadata.hermes.category".into(),
            ));
        }
    }
    let mut seen = HashSet::new();
    for selector in &related {
        validate_portable_name(selector)?;
        if selector == skill_name {
            return Err(SkillError::Invalid(format!(
                "skill {skill_name} cannot relate to itself"
            )));
        }
        if !seen.insert(selector.clone()) {
            return Err(SkillError::Invalid(format!(
                "duplicate related skill: {selector}"
            )));
        }
    }
    related.sort();
    Ok((top_category.or(nested_category), related))
}

fn yaml_relationship_list(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
) -> Result<Vec<String>, SkillError> {
    let Some(value) = yaml_value(mapping, key) else {
        return Ok(Vec::new());
    };
    if value.is_sequence() {
        return yaml_string_list(mapping, key, 16, 64);
    }
    let raw = value
        .as_str()
        .ok_or_else(|| SkillError::Invalid(format!("{key} must be a string or list")))?
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let values = raw
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if values.len() > 16 {
        return Err(SkillError::Invalid(format!(
            "{key} contains more than 16 entries"
        )));
    }
    if values
        .iter()
        .any(|value| value.is_empty() || value.chars().count() > 64)
    {
        return Err(SkillError::Invalid(format!(
            "{key} entries must contain 1 to 64 characters"
        )));
    }
    Ok(values)
}

fn normalize_skill_category(value: &str) -> Result<String, SkillError> {
    let normalized = value.trim().to_ascii_lowercase();
    let valid = normalized
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_/".contains(&byte));
    if !valid
        || normalized.starts_with('/')
        || normalized.ends_with('/')
        || normalized.contains("//")
    {
        return Err(SkillError::Invalid(
            "category must use lowercase letters, digits, '-', '_', or single '/' separators"
                .into(),
        ));
    }
    Ok(normalized)
}

pub(crate) fn validate_setting_key(key: &str) -> Result<(), SkillError> {
    let lower = key.trim().to_ascii_lowercase();
    if lower.is_empty()
        || lower.len() > 64
        || !lower
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(SkillError::Invalid(format!(
            "invalid skill config key: {key}"
        )));
    }
    if [
        "secret",
        "token",
        "password",
        "credential",
        "api_key",
        "apikey",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err(SkillError::Invalid(format!(
            "credential-like skill config is not allowed: {key}"
        )));
    }
    Ok(())
}

pub fn parse_setting_assignment(value: &str) -> Result<SkillSettingValue, SkillError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 8 * 1024 {
        return Err(SkillError::Invalid(
            "skill config value must contain 1 to 8192 bytes".into(),
        ));
    }
    if value.eq_ignore_ascii_case("true") {
        return Ok(SkillSettingValue::Boolean(true));
    }
    if value.eq_ignore_ascii_case("false") {
        return Ok(SkillSettingValue::Boolean(false));
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Ok(SkillSettingValue::Integer(integer));
    }
    if value.starts_with('[') {
        let parsed = serde_json::from_str::<Vec<String>>(value).map_err(|_| {
            SkillError::Invalid("skill config lists must be JSON string arrays".into())
        })?;
        if parsed.len() > 32 {
            return Err(SkillError::Invalid(
                "skill config list contains more than 32 values".into(),
            ));
        }
        let parsed = SkillSettingValue::Strings(parsed);
        validate_setting_value_safety(&parsed)?;
        return Ok(parsed);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("sk-") || lower.starts_with("ghp_") || lower.starts_with("bearer ") {
        return Err(SkillError::Invalid(
            "credential-like skill config values are not allowed".into(),
        ));
    }
    let parsed = SkillSettingValue::String(value.to_string());
    validate_setting_value_safety(&parsed)?;
    Ok(parsed)
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceSkillConfigFile {
    version: u32,
    #[serde(default)]
    values: BTreeMap<String, SkillSettingValue>,
}

pub fn load_workspace_skill_config(
    path: &Path,
) -> Result<BTreeMap<String, SkillSettingValue>, SkillError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| SkillError::Io(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(SkillError::Invalid(
            "workspace skill config must be a bounded regular file".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
            return Err(SkillError::Invalid(
                "workspace skill config must be an owner-only single-link file".into(),
            ));
        }
    }
    let file: WorkspaceSkillConfigFile =
        serde_json::from_slice(&fs::read(path).map_err(|error| SkillError::Io(error.to_string()))?)
            .map_err(|error| SkillError::Invalid(error.to_string()))?;
    if file.version != 1 || file.values.len() > 256 {
        return Err(SkillError::Invalid(
            "workspace skill config has an unsupported shape".into(),
        ));
    }
    for (key, value) in &file.values {
        let (selector, setting) = key.rsplit_once('.').ok_or_else(|| {
            SkillError::Invalid("workspace skill config key must be <skill>.<setting>".into())
        })?;
        if selector.is_empty() || key.len() > 512 {
            return Err(SkillError::Invalid(
                "workspace skill config selector is invalid".into(),
            ));
        }
        validate_setting_key(setting)?;
        validate_setting_value_safety(value)?;
    }
    Ok(file.values)
}

pub fn save_workspace_skill_config(
    path: &Path,
    key: &str,
    value: SkillSettingValue,
) -> Result<(), SkillError> {
    let (_, setting) = key
        .rsplit_once('.')
        .ok_or_else(|| SkillError::Invalid("skill config key must be <skill>.<setting>".into()))?;
    validate_setting_key(setting)?;
    validate_setting_value_safety(&value)?;
    let mut values = load_workspace_skill_config(path)?;
    values.insert(key.to_string(), value);
    if values.len() > 256 {
        return Err(SkillError::Invalid(
            "workspace skill config contains more than 256 values".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| SkillError::Invalid("workspace skill config has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|error| SkillError::Io(error.to_string()))?;
    let temp = parent.join(format!(".skill-config-{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(&WorkspaceSkillConfigFile { version: 1, values })
        .map_err(|error| SkillError::Invalid(error.to_string()))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), SkillError> {
        let mut file = options
            .open(&temp)
            .map_err(|error| SkillError::Io(error.to_string()))?;
        file.write_all(&bytes)
            .map_err(|error| SkillError::Io(error.to_string()))?;
        file.sync_all()
            .map_err(|error| SkillError::Io(error.to_string()))?;
        fs::rename(&temp, path).map_err(|error| SkillError::Io(error.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn parse_setting_value(value: &serde_yaml_ng::Value) -> Result<SkillSettingValue, SkillError> {
    let parsed = if let Some(value) = value.as_str() {
        let lower = value.trim().to_ascii_lowercase();
        if lower.starts_with("sk-") || lower.starts_with("ghp_") || lower.starts_with("bearer ") {
            return Err(SkillError::Invalid(
                "credential-like skill config defaults are not allowed".into(),
            ));
        }
        SkillSettingValue::String(value.to_string())
    } else if let Some(value) = value.as_i64() {
        SkillSettingValue::Integer(value)
    } else if let Some(value) = value.as_bool() {
        SkillSettingValue::Boolean(value)
    } else if let Some(values) = value.as_sequence() {
        if values.len() > 32 {
            return Err(SkillError::Invalid(
                "skill config list contains more than 32 values".into(),
            ));
        }
        SkillSettingValue::Strings(
            values
                .iter()
                .map(|value| {
                    value.as_str().map(str::to_string).ok_or_else(|| {
                        SkillError::Invalid("skill config lists must contain only strings".into())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        return Err(SkillError::Invalid(
            "skill config values must be bounded scalars or string lists".into(),
        ));
    };
    let encoded =
        serde_json::to_vec(&parsed).map_err(|error| SkillError::Invalid(error.to_string()))?;
    if encoded.len() > 8 * 1024 {
        return Err(SkillError::Invalid(
            "skill config value exceeds its byte bound".into(),
        ));
    }
    validate_setting_value_safety(&parsed)?;
    Ok(parsed)
}

pub(crate) fn validate_setting_value_safety(value: &SkillSettingValue) -> Result<(), SkillError> {
    let strings = match value {
        SkillSettingValue::String(value) => std::slice::from_ref(value),
        SkillSettingValue::Strings(values) => values.as_slice(),
        SkillSettingValue::Integer(_) | SkillSettingValue::Boolean(_) => return Ok(()),
    };
    if strings.iter().any(|value| {
        let lower = value.trim().to_ascii_lowercase();
        value.is_empty()
            || value.len() > 8 * 1024
            || lower.starts_with("sk-")
            || lower.starts_with("ghp_")
            || lower.starts_with("github_pat_")
            || lower.starts_with("xoxb-")
            || lower.starts_with("xoxp-")
            || lower.starts_with("bearer ")
            || lower.starts_with("authorization:")
            || lower.starts_with("api_key=")
            || lower.starts_with("apikey=")
            || lower.starts_with("token=")
            || lower.starts_with("password=")
            || crate::learning::sanitized_learning_text(value).as_deref() != Some(value.as_str())
    }) {
        return Err(SkillError::Invalid(
            "credential-like or unsafe skill config values are not allowed".into(),
        ));
    }
    Ok(())
}

fn yaml_mapping<'a>(
    value: &'a serde_yaml_ng::Value,
    context: &str,
) -> Result<&'a serde_yaml_ng::Mapping, SkillError> {
    value
        .as_mapping()
        .ok_or_else(|| SkillError::Invalid(format!("{context} must be a mapping")))
}

fn yaml_value<'a>(
    mapping: &'a serde_yaml_ng::Mapping,
    key: &str,
) -> Option<&'a serde_yaml_ng::Value> {
    mapping.get(serde_yaml_ng::Value::String(key.to_string()))
}

fn yaml_required_string(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    maximum_chars: usize,
) -> Result<String, SkillError> {
    yaml_optional_string(mapping, key, maximum_chars)?
        .ok_or_else(|| SkillError::Invalid(format!("{key} is required")))
}

fn yaml_optional_string(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    maximum_chars: usize,
) -> Result<Option<String>, SkillError> {
    let Some(value) = yaml_value(mapping, key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| SkillError::Invalid(format!("{key} must be a string")))?
        .trim();
    if value.is_empty() || value.chars().count() > maximum_chars {
        return Err(SkillError::Invalid(format!(
            "{key} must contain 1 to {maximum_chars} characters"
        )));
    }
    Ok(Some(value.to_string()))
}

fn yaml_string_list(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    maximum_entries: usize,
    maximum_chars: usize,
) -> Result<Vec<String>, SkillError> {
    yaml_string_list_with_case(mapping, key, maximum_entries, maximum_chars, false)
}

fn yaml_case_sensitive_string_list(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    maximum_entries: usize,
    maximum_chars: usize,
) -> Result<Vec<String>, SkillError> {
    yaml_string_list_with_case(mapping, key, maximum_entries, maximum_chars, true)
}

fn yaml_string_list_with_case(
    mapping: &serde_yaml_ng::Mapping,
    key: &str,
    maximum_entries: usize,
    maximum_chars: usize,
    preserve_case: bool,
) -> Result<Vec<String>, SkillError> {
    let Some(value) = yaml_value(mapping, key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_sequence()
        .ok_or_else(|| SkillError::Invalid(format!("{key} must be a list")))?;
    if values.len() > maximum_entries {
        return Err(SkillError::Invalid(format!(
            "{key} contains more than {maximum_entries} entries"
        )));
    }
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| SkillError::Invalid(format!("{key} entries must be strings")))?
                .trim();
            let value = if preserve_case {
                value.to_string()
            } else {
                value.to_ascii_lowercase()
            };
            if value.is_empty() || value.chars().count() > maximum_chars {
                return Err(SkillError::Invalid(format!(
                    "{key} entries must contain 1 to {maximum_chars} characters"
                )));
            }
            Ok(value)
        })
        .collect()
}

fn reject_unknown_yaml_keys(
    mapping: &serde_yaml_ng::Mapping,
    allowed: &[&str],
    context: &str,
) -> Result<(), SkillError> {
    for key in mapping.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| SkillError::Invalid(format!("{context} keys must be strings")))?;
        if !allowed.contains(&key) {
            return Err(SkillError::Invalid(format!(
                "unknown {context} field: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_portable_name(value: &str) -> Result<(), SkillError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && !value.contains("--");
    if !valid {
        return Err(SkillError::Invalid(format!(
            "invalid portable skill name: {value}"
        )));
    }
    Ok(())
}

fn collect_candidates(
    root: &Path,
    boundary: Option<&Path>,
    origin: SkillOrigin,
    disabled: &HashSet<String>,
    canonical_files: &mut HashSet<PathBuf>,
    candidates: &mut Vec<Candidate>,
    diagnostics: &mut Vec<String>,
) {
    if !root.exists() {
        return;
    }
    if std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        diagnostics.push(format!(
            "{}: symlinked skill roots are ignored",
            root.display()
        ));
        return;
    }
    let canonical_root = match std::fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => {
            diagnostics.push(format!("{}: {error}", root.display()));
            return;
        }
    };
    if let Some(boundary) = boundary {
        let boundary = match std::fs::canonicalize(boundary) {
            Ok(boundary) => boundary,
            Err(error) => {
                diagnostics.push(format!("{}: {error}", boundary.display()));
                return;
            }
        };
        if !canonical_root.starts_with(&boundary) {
            diagnostics.push(format!(
                "{}: skill root escapes project workspace",
                root.display()
            ));
            return;
        }
    }
    let mut directories = vec![canonical_root.clone()];
    while let Some(directory) = directories.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(format!("{}: {error}", directory.display()));
                continue;
            }
        };
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    diagnostics.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                diagnostics.push(format!(
                    "{}: symlinked skill entries are ignored",
                    path.display()
                ));
                continue;
            }
            if metadata.is_dir() {
                directories.push(path);
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
                continue;
            }
            let canonical = match std::fs::canonicalize(&path) {
                Ok(path) if path.starts_with(&canonical_root) => path,
                Ok(path) => {
                    diagnostics.push(format!("{}: skill escapes discovery root", path.display()));
                    continue;
                }
                Err(error) => {
                    diagnostics.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            if !canonical_files.insert(canonical.clone()) {
                diagnostics.push(format!(
                    "{}: duplicate canonical skill file",
                    canonical.display()
                ));
                continue;
            }
            if metadata.len() > MAX_SKILL_BYTES {
                diagnostics.push(format!("{}: skill exceeds 128 KiB", canonical.display()));
                continue;
            }
            match load_skill(&canonical) {
                Ok((parsed, _sha256)) if disabled.contains(&parsed.id) => diagnostics.push(
                    format!("{}: skill {} is disabled", canonical.display(), parsed.id),
                ),
                Ok((parsed, sha256)) => candidates.push(Candidate {
                    metadata: SkillMetadata {
                        id: parsed.id,
                        description: parsed.description,
                        aliases: parsed.aliases,
                        path: canonical,
                        sha256,
                        origin,
                    },
                }),
                Err(error) => diagnostics.push(format!("{}: {error}", canonical.display())),
            }
        }
    }
}

fn load_skill(path: &Path) -> Result<(ParsedSkill, String), SkillError> {
    let metadata = std::fs::metadata(path).map_err(|error| SkillError::Io(error.to_string()))?;
    if metadata.len() > MAX_SKILL_BYTES {
        return Err(SkillError::Invalid("skill exceeds 128 KiB".into()));
    }
    let bytes = std::fs::read(path).map_err(|error| SkillError::Io(error.to_string()))?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| SkillError::Invalid("skill is not valid UTF-8".into()))?;
    Ok((parse_skill(text)?, sha256))
}

fn parse_skill(text: &str) -> Result<ParsedSkill, SkillError> {
    let normalized = text.replace("\r\n", "\n");
    let mut lines = normalized.lines();
    if lines.next() != Some("---") {
        return Err(SkillError::Invalid(
            "missing opening frontmatter delimiter".into(),
        ));
    }
    let mut name = None;
    let mut description = None;
    let mut aliases = Vec::new();
    let mut aliases_block = false;
    let mut body_lines = Vec::new();
    let mut closed = false;
    for line in &mut lines {
        if line == "---" {
            closed = true;
            body_lines.extend(lines);
            break;
        }
        if aliases_block && line.trim_start().starts_with('-') {
            let alias = parse_scalar(line.trim_start().trim_start_matches('-').trim())?
                .to_ascii_lowercase();
            aliases.push(alias);
            continue;
        }
        aliases_block = false;
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| SkillError::Invalid(format!("invalid frontmatter line: {line}")))?;
        match key.trim() {
            "name" if name.is_none() => name = Some(parse_scalar(value)?.to_ascii_lowercase()),
            "description" if description.is_none() => description = Some(parse_scalar(value)?),
            "aliases" if aliases.is_empty() => {
                let value = value.trim();
                if value.is_empty() {
                    aliases_block = true;
                } else if value.starts_with('[') && value.ends_with(']') {
                    let inner = &value[1..value.len() - 1];
                    if !inner.trim().is_empty() {
                        for alias in inner.split(',') {
                            aliases.push(parse_scalar(alias)?.to_ascii_lowercase());
                        }
                    }
                } else {
                    return Err(SkillError::Invalid(
                        "aliases must be an inline or block list".into(),
                    ));
                }
            }
            "name" | "description" | "aliases" => {
                return Err(SkillError::Invalid(format!(
                    "duplicate field: {}",
                    key.trim()
                )));
            }
            unknown => {
                return Err(SkillError::Invalid(format!(
                    "unknown frontmatter field: {unknown}"
                )));
            }
        }
    }
    if !closed {
        return Err(SkillError::Invalid(
            "missing closing frontmatter delimiter".into(),
        ));
    }
    let id = name.ok_or_else(|| SkillError::Invalid("name is required".into()))?;
    let description =
        description.ok_or_else(|| SkillError::Invalid("description is required".into()))?;
    validate_identity(&id)?;
    if description.is_empty() || description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(SkillError::Invalid(
            "description must contain 1 to 1024 characters".into(),
        ));
    }
    if aliases.len() > 16 {
        return Err(SkillError::Invalid("at most 16 aliases are allowed".into()));
    }
    let mut seen = HashSet::from([id.clone()]);
    for alias in &aliases {
        validate_identity(alias)?;
        if !seen.insert(alias.clone()) {
            return Err(SkillError::Invalid(format!("duplicate identity: {alias}")));
        }
    }
    let body = body_lines.join("\n").trim().to_string();
    if body.is_empty() {
        return Err(SkillError::Invalid("skill instructions are empty".into()));
    }
    Ok(ParsedSkill {
        id,
        description,
        aliases,
        body,
    })
}

fn parse_scalar(value: &str) -> Result<String, SkillError> {
    let value = value.trim();
    let value = if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        &value[1..value.len() - 1]
    } else {
        value
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(SkillError::Invalid("frontmatter value is empty".into()));
    }
    Ok(value)
}

fn validate_identity(value: &str) -> Result<(), SkillError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'-' | b'_' => index > 0,
            _ => false,
        });
    if !valid {
        return Err(SkillError::Invalid(format!(
            "invalid skill identity: {value}"
        )));
    }
    Ok(())
}

fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(
        root: &Path,
        directory: &str,
        name: &str,
        description: &str,
        body: &str,
    ) -> PathBuf {
        let directory = root.join(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("SKILL.md");
        std::fs::write(
            &path,
            format!(
                "---\nname: {name}\ndescription: {description}\naliases: [{name}-alias]\n---\n{body}\n"
            ),
        )
        .unwrap();
        path
    }

    fn roots(project: &Path, user: &Path, compatibility: Vec<PathBuf>) -> SkillRoots {
        SkillRoots {
            project: Some(project.to_path_buf()),
            project_boundary: Some(project.to_path_buf()),
            user: Some(user.to_path_buf()),
            compatibility,
        }
    }

    #[test]
    fn project_precedes_user_and_compatibility_is_opt_in() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let user = temp.path().join("user");
        let compatibility = temp.path().join("compatibility");
        write_skill(
            &project,
            "project-foo",
            "foo",
            "Project Description",
            "project body",
        );
        write_skill(&user, "user-foo", "foo", "User Description", "user body");
        write_skill(&user, "bar", "bar", "User Bar", "bar body");
        write_skill(&compatibility, "baz", "baz", "Compat Baz", "baz body");

        let without_compat = SkillRegistry::discover(roots(&project, &user, Vec::new()), &[]);
        assert_eq!(
            without_compat
                .entries()
                .map(|skill| skill.id.as_str())
                .collect::<Vec<_>>(),
            ["bar", "foo"]
        );
        assert_eq!(
            without_compat.entries.get("foo").unwrap().origin,
            SkillOrigin::Project
        );
        assert!(!without_compat.entries.contains_key("baz"));
        assert!(without_compat
            .diagnostics()
            .iter()
            .any(|line| line.contains("shadowed")));

        let with_compat = SkillRegistry::discover(roots(&project, &user, vec![compatibility]), &[]);
        assert!(with_compat.entries.contains_key("baz"));
    }

    #[cfg(unix)]
    #[test]
    fn malformed_duplicate_oversize_and_symlinked_skills_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        write_skill(&project, "duplicate-a", "duplicate", "First", "body");
        write_skill(&project, "duplicate-b", "duplicate", "Second", "body");
        let invalid = project.join("invalid");
        std::fs::create_dir_all(&invalid).unwrap();
        std::fs::write(
            invalid.join("SKILL.md"),
            "---\nname: invalid\ndescription: invalid\nunknown: field\n---\nbody",
        )
        .unwrap();
        let oversize = project.join("oversize");
        std::fs::create_dir_all(&oversize).unwrap();
        std::fs::write(
            oversize.join("SKILL.md"),
            vec![b'x'; MAX_SKILL_BYTES as usize + 1],
        )
        .unwrap();
        let outside = temp.path().join("outside-SKILL.md");
        std::fs::write(
            &outside,
            "---\nname: outside\ndescription: outside\n---\nsecret body",
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, project.join("SKILL.md")).unwrap();

        let registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(project),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        assert!(registry.entries().next().is_none());
        let diagnostics = registry.diagnostics().join("\n");
        assert!(diagnostics.contains("duplicate project skill identity duplicate"));
        assert!(diagnostics.contains("unknown frontmatter field"));
        assert!(diagnostics.contains("exceeds 128 KiB"));
        assert!(diagnostics.contains("symlinked skill entries are ignored"));
        assert!(!diagnostics.contains("secret body"));
    }

    #[cfg(unix)]
    #[test]
    fn project_skill_root_cannot_escape_through_a_symlinked_ca_directory() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        write_skill(
            &outside,
            "skills/deploy",
            "deploy",
            "Deploy",
            "outside body",
        );
        std::os::unix::fs::symlink(&outside, workspace.join(".ca")).unwrap();

        let registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(workspace.join(".ca/skills")),
                project_boundary: Some(workspace),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        assert!(registry.entries().next().is_none());
        assert!(registry
            .diagnostics()
            .iter()
            .any(|line| line.contains("escapes project workspace")));
    }

    #[test]
    fn catalog_is_metadata_only_and_activation_is_hash_bound() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let path = write_skill(
            &project,
            "deploy",
            "deploy",
            "Deploy Carefully",
            "PRIVATE-INSTRUCTION-BODY",
        );
        let mut registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(project.clone()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        assert!(registry.catalog_prompt().contains("Deploy Carefully"));
        assert!(registry
            .catalog_prompt()
            .contains("activate the exact relevant skill before proceeding"));
        assert!(!registry
            .catalog_prompt()
            .contains("PRIVATE-INSTRUCTION-BODY"));
        assert_eq!(registry.search("deploy-alias")[0].id, "deploy");
        assert!(matches!(
            registry.activate_exact("deploy-alias"),
            Err(SkillError::Unknown(_))
        ));
        registry.activate_exact("deploy").unwrap();
        assert!(registry
            .active_prompt()
            .contains("PRIVATE-INSTRUCTION-BODY"));
        let references = registry.active_refs();
        assert_eq!(references.len(), 1);

        std::fs::write(
            &path,
            "---\nname: deploy\ndescription: Deploy Carefully\n---\nCHANGED",
        )
        .unwrap();
        assert!(matches!(
            registry.view_exact("deploy"),
            Err(SkillError::Changed(_))
        ));
        let mut refreshed = SkillRegistry::discover(
            SkillRoots {
                project: Some(project),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );
        let warnings = refreshed.restore(&references);
        assert_eq!(
            warnings,
            ["skill deploy changed and requires reconfirmation"]
        );
        assert!(!refreshed.is_active("deploy"));
    }

    #[test]
    fn active_instruction_budget_and_disabled_ids_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        write_skill(&project, "large", "large", "Large", &"x".repeat(48_001));
        write_skill(&project, "disabled", "disabled", "Disabled", "body");
        let mut registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(project),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &["disabled".into()],
        );

        assert!(matches!(
            registry.activate_exact("large"),
            Err(SkillError::BudgetExceeded)
        ));
        assert!(matches!(
            registry.activate_exact("disabled"),
            Err(SkillError::Disabled(_))
        ));
    }

    #[test]
    fn model_catalog_has_entry_and_character_bounds() {
        let temp = tempfile::tempdir().unwrap();
        for index in 0..80 {
            write_skill(
                temp.path(),
                &format!("skill-{index:02}"),
                &format!("skill-{index:02}"),
                &"description ".repeat(40),
                "body",
            );
        }
        let registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(temp.path().to_path_buf()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );
        let catalog = registry.catalog_prompt();

        assert!(catalog.len() <= MAX_CATALOG_CHARS);
        assert!(
            catalog
                .lines()
                .filter(|line| line.starts_with("- "))
                .count()
                <= MAX_CATALOG_ENTRIES
        );
        assert!(!catalog.contains("\nbody\n"));
    }

    #[test]
    fn strict_frontmatter_supports_crlf_and_block_aliases_without_lowercasing_description() {
        let parsed = parse_skill(
            "---\r\nname: Example\r\ndescription: Keep Case\r\naliases:\r\n  - EX\r\n---\r\nDo the work.\r\n",
        )
        .unwrap();
        assert_eq!(parsed.id, "example");
        assert_eq!(parsed.description, "Keep Case");
        assert_eq!(parsed.aliases, ["ex"]);
        assert_eq!(parsed.body, "Do the work.");
    }

    #[test]
    fn suggestions_rank_exact_id_then_alias_then_bm25_metadata() {
        let temp = tempfile::tempdir().unwrap();
        write_skill(
            temp.path(),
            "release",
            "release-manager",
            "Coordinate a safe production deployment and rollback",
            "release body",
        );
        write_skill(
            temp.path(),
            "review",
            "rust-review",
            "Review Rust code for correctness and maintainability",
            "review body",
        );
        let registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(temp.path().to_path_buf()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        let exact = registry.suggest("please use release-manager for this", 3);
        assert_eq!(exact[0].id, "release-manager");
        assert_eq!(exact[0].match_kind, SkillMatchKind::ExactId);

        let alias = registry.suggest("use release-manager-alias for this", 3);
        assert_eq!(alias[0].id, "release-manager");
        assert_eq!(alias[0].match_kind, SkillMatchKind::ExactAlias);
        assert_eq!(
            alias[0].matched_alias.as_deref(),
            Some("release-manager-alias")
        );

        let related = registry.suggest("check maintainability and Rust correctness", 3);
        assert_eq!(related[0].id, "rust-review");
        assert_eq!(related[0].match_kind, SkillMatchKind::Related);
        assert!(related[0].relevance > 0);
    }

    #[test]
    fn suggestions_are_unicode_aware_bounded_stable_and_noise_filtered() {
        let temp = tempfile::tempdir().unwrap();
        write_skill(
            temp.path(),
            "analytics",
            "data-analytics",
            "Analyse des donnees et creer des rapports detailles",
            "analytics body",
        );
        for index in 0..8 {
            write_skill(
                temp.path(),
                &format!("report-{index}"),
                &format!("report-{index}"),
                "Create a detailed report from project evidence",
                "report body",
            );
        }
        let registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(temp.path().to_path_buf()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        let unicode = registry.suggest("ANALYSE donnees", 3);
        assert_eq!(unicode[0].id, "data-analytics");
        assert_eq!(unicode[0].match_kind, SkillMatchKind::Related);

        let bounded = registry.suggest("detailed report project evidence", usize::MAX);
        assert_eq!(bounded.len(), MAX_SKILL_SUGGESTIONS);
        assert_eq!(
            bounded
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["report-0", "report-1", "report-2", "report-3", "report-4"]
        );
        assert!(registry.suggest("the and with to", 3).is_empty());
        assert!(registry.suggest("---", 3).is_empty());
    }

    #[test]
    fn suggestions_never_load_bodies_or_relax_exact_hash_bound_activation() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            temp.path(),
            "deploy",
            "deploy",
            "Deploy a release safely",
            "PRIVATE-BODY",
        );
        let mut registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(temp.path().to_path_buf()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );

        let catalog_before = registry.catalog_prompt();
        let active_before = registry.active_refs();
        let suggestion = registry.suggest("ship a safe release", 3).remove(0);
        assert_eq!(suggestion.id, "deploy");
        assert!(!format!("{suggestion:?}").contains("PRIVATE-BODY"));
        assert_eq!(registry.catalog_prompt(), catalog_before);
        assert_eq!(registry.active_refs(), active_before);
        assert!(matches!(
            registry.activate_exact("deploy-alias"),
            Err(SkillError::Unknown(_))
        ));

        std::fs::write(
            path,
            "---\nname: deploy\ndescription: Deploy a release safely\n---\nCHANGED-BODY",
        )
        .unwrap();
        assert_eq!(registry.suggest("deploy", 3)[0].id, "deploy");
        assert!(matches!(
            registry.activate_exact("deploy"),
            Err(SkillError::Changed(_))
        ));
    }

    #[test]
    fn reload_rebuilds_the_metadata_index_without_auto_activating_new_matches() {
        let temp = tempfile::tempdir().unwrap();
        write_skill(
            temp.path(),
            "review",
            "rust-review",
            "Review Rust correctness",
            "review body",
        );
        let mut registry = SkillRegistry::discover(
            SkillRoots {
                project: Some(temp.path().to_path_buf()),
                project_boundary: Some(temp.path().to_path_buf()),
                user: None,
                compatibility: Vec::new(),
            },
            &[],
        );
        assert!(registry.suggest("production deployment", 3).is_empty());

        write_skill(
            temp.path(),
            "release",
            "release-manager",
            "Coordinate a production deployment",
            "release body",
        );
        assert!(registry.suggest("production deployment", 3).is_empty());
        registry.reload();

        assert_eq!(
            registry.suggest("production deployment", 3)[0].id,
            "release-manager"
        );
        assert!(registry.active_refs().is_empty());
    }

    fn write_portable_skill(root: &Path, name: &str, description: &str) -> PathBuf {
        let directory = root.join(name);
        std::fs::create_dir_all(directory.join("references")).unwrap();
        std::fs::create_dir_all(directory.join("scripts")).unwrap();
        std::fs::create_dir_all(directory.join("agents")).unwrap();
        std::fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  author: example\nallowed-tools: Bash(git:*) Read\n---\nUse references as needed.\n"
            ),
        )
        .unwrap();
        std::fs::write(directory.join("references/guide.md"), "guide-v1\n").unwrap();
        std::fs::write(directory.join("scripts/check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(
            directory.join("agents/openai.yaml"),
            "policy:\n  allow_implicit_invocation: false\ndependencies:\n  tools:\n    - type: mcp\n      value: docs\n      description: Documentation server\n      transport: streamable_http\n      url: https://example.invalid/mcp\n",
        )
        .unwrap();
        directory
    }

    #[test]
    fn portable_instances_preserve_duplicates_and_unqualified_resolution_is_ambiguous() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let user = temp.path().join("user");
        write_portable_skill(&workspace, "deploy", "Deploy workspace services");
        write_portable_skill(&user, "deploy", "Deploy personal services");

        let registry = SkillRegistry::discover_portable(
            vec![
                SkillDiscoveryRoot::new(
                    SkillSourceScope::Workspace,
                    workspace,
                    "root-.agents-skills",
                ),
                SkillDiscoveryRoot::new(SkillSourceScope::User, user, "user-skills"),
            ],
            &[],
        );
        let instances = registry.instances().collect::<Vec<_>>();
        assert_eq!(instances.len(), 2);
        assert_ne!(instances[0].instance_id, instances[1].instance_id);
        assert!(instances
            .iter()
            .all(|instance| instance.qualified_name.ends_with("::deploy")));
        assert!(matches!(
            registry.resolve_instance("deploy", true),
            Err(SkillError::Ambiguous { .. })
        ));
        for instance in instances {
            assert_eq!(
                registry
                    .resolve_instance(&instance.qualified_name, true)
                    .unwrap()
                    .instance_id,
                instance.instance_id
            );
        }
    }

    #[test]
    fn workspace_context_can_be_hidden_without_disabling_explicit_skills() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let user = temp.path().join("user");
        write_portable_skill(&workspace, "project-check", "Check this project");
        write_portable_skill(&user, "personal-check", "Check personal settings");

        let mut registry = SkillRegistry::discover_portable(
            vec![
                SkillDiscoveryRoot::new(SkillSourceScope::Workspace, workspace, "workspace"),
                SkillDiscoveryRoot::new(SkillSourceScope::User, user, "user"),
            ],
            &[],
        );
        let project_selector = registry
            .instances()
            .find(|instance| instance.metadata.name == "project-check")
            .unwrap()
            .qualified_name
            .clone();

        registry.hide_workspace_context();

        assert_eq!(
            registry
                .instances()
                .map(|instance| instance.metadata.name.as_str())
                .collect::<Vec<_>>(),
            ["personal-check"]
        );
        assert!(!registry
            .portable_catalog_prompt(None)
            .contains("project-check"));
        assert!(!registry.activation_selectors().contains(&project_selector));
        let mut loads = SkillLoadSet::default();
        registry
            .load_for_task(&project_selector, true, &mut loads)
            .unwrap();
        assert!(registry
            .task_prompt(&loads)
            .unwrap()
            .contains("project-check"));
    }

    #[test]
    fn portable_relationship_metadata_is_bounded_normalized_and_non_authorizing() {
        let parsed = parse_portable_skill(
            "---\nname: release\ndescription: Release safely\ncategory: Delivery/Release\nrelated_skills:\n  - review\n  - deploy\nmetadata:\n  hermes:\n    related_skills:\n      - test\n---\nUse the release checklist.\n",
        )
        .unwrap();
        assert_eq!(
            parsed.metadata.category.as_deref(),
            Some("delivery/release")
        );
        assert_eq!(parsed.metadata.related_skills, ["deploy", "review", "test"]);
        assert!(parsed.metadata.allowed_tools.is_none());
        let scalar = parse_portable_skill(
            "---\nname: release\ndescription: Release safely\nmetadata:\n  hermes:\n    related_skills: review, deploy\n---\nUse the release checklist.\n",
        )
        .unwrap();
        assert_eq!(scalar.metadata.related_skills, ["deploy", "review"]);

        let duplicate = parse_portable_skill(
            "---\nname: release\ndescription: Release safely\nrelated_skills: [review, REVIEW]\n---\nUse the release checklist.\n",
        )
        .unwrap_err()
        .to_string();
        assert!(duplicate.contains("duplicate related skill"));

        let self_reference = parse_portable_skill(
            "---\nname: release\ndescription: Release safely\nrelated_skills: [release]\n---\nUse the release checklist.\n",
        )
        .unwrap_err()
        .to_string();
        assert!(self_reference.contains("cannot relate to itself"));

        let oversized = (0..17)
            .map(|index| format!("skill-{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let error = parse_portable_skill(&format!(
            "---\nname: release\ndescription: Release safely\nrelated_skills: [{oversized}]\n---\nUse the release checklist.\n"
        ))
        .unwrap_err()
        .to_string();
        assert!(error.contains("more than 16 entries"));

        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("release");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            "---\nname: release\ndescription: Release safely\ncategory: delivery\nrelated_skills: [review]\n---\nUse the release checklist.\n",
        )
        .unwrap();
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                temp.path(),
                "test",
            )],
            &[],
        );
        let catalog = registry.portable_catalog_prompt(None);
        assert!(catalog.contains("category: delivery; related: review"));
        assert!(!catalog.contains("Use the release checklist"));
    }

    #[test]
    fn portable_tree_digest_covers_resources_policy_and_declared_authority_is_inert() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        let directory = write_portable_skill(&root, "review", "Review code safely");
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root.clone(),
                "repo-skills",
            )],
            &[],
        );
        let instance = registry.instances().next().unwrap();
        assert!(!instance.metadata.allow_implicit_invocation);
        assert_eq!(
            instance.metadata.allowed_tools.as_deref(),
            Some("Bash(git:*) Read")
        );
        assert_eq!(instance.metadata.dependencies.len(), 1);
        assert!(instance
            .files
            .iter()
            .any(|file| file.relative_path == Path::new("references/guide.md")));
        let id = instance.instance_id.clone();
        let digest = instance.content_digest.clone();
        assert_eq!(
            registry.skill_view(&id, "references/guide.md").unwrap(),
            "guide-v1\n"
        );

        std::fs::write(directory.join("references/guide.md"), "guide-v2\n").unwrap();
        assert!(matches!(
            registry.skill_view(&id, "references/guide.md"),
            Err(SkillError::Changed(_))
        ));
        let refreshed = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root,
                "repo-skills",
            )],
            &[],
        );
        let refreshed = refreshed.instances().next().unwrap();
        assert_ne!(refreshed.content_digest, digest);
        assert_ne!(refreshed.instance_id, id);
    }

    #[test]
    fn explicit_task_loads_are_local_pinned_and_implicit_policy_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        write_portable_skill(&root, "review", "Review code safely");
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root,
                "repo",
            )],
            &[],
        );
        assert!(matches!(
            registry.resolve_instance("review", false),
            Err(SkillError::Unknown(_))
        ));
        let mut first = SkillLoadSet::default();
        registry.load_for_task("review", true, &mut first).unwrap();
        assert_eq!(first.len(), 1);
        assert!(registry
            .task_prompt(&first)
            .unwrap()
            .contains("Use references as needed."));
        let second = SkillLoadSet::default();
        assert!(second.is_empty());
        assert_eq!(
            explicit_skill_selectors("please use $review, then $review"),
            ["review"]
        );

        let mut persistent = registry.clone();
        let qualified = persistent
            .instances()
            .next()
            .unwrap()
            .qualified_name
            .clone();
        persistent.activate_exact(&qualified).unwrap();
        let references = persistent.active_refs();
        let mut restored = persistent.clone();
        restored.active.clear();
        assert!(restored.restore(&references).is_empty());
        assert!(restored.is_active(&qualified));
    }

    #[test]
    fn pure_pin_resolution_never_falls_back_to_a_name_or_changed_digest() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        write_portable_skill(&root, "review", "Review code safely");
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root,
                "repo",
            )],
            &[],
        );
        let instance = registry.instances().next().unwrap();
        assert_eq!(
            resolve_pinned_instance(
                registry.instances(),
                &instance.instance_id,
                &instance.content_digest
            )
            .unwrap()
            .instance_id,
            instance.instance_id
        );
        assert!(matches!(
            resolve_pinned_instance(registry.instances(), "review", &instance.content_digest),
            Err(SkillError::Unknown(_))
        ));
        assert!(matches!(
            resolve_pinned_instance(registry.instances(), &instance.instance_id, "changed"),
            Err(SkillError::Changed(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn portable_resource_escape_and_inventory_bounds_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        let escaped = write_portable_skill(&root, "escaped", "Escaped resource");
        let outside = temp.path().join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, escaped.join("references/outside.md")).unwrap();
        let crowded = write_portable_skill(&root, "crowded", "Crowded resource");
        for index in 0..MAX_SKILL_TREE_FILES {
            std::fs::write(crowded.join(format!("references/{index:03}.md")), "bounded").unwrap();
        }
        let registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root,
                "repo",
            )],
            &[],
        );
        assert!(registry.instances().next().is_none());
        let diagnostics = registry.diagnostics().join("\n");
        assert!(diagnostics.contains("escapes its root"));
        assert!(diagnostics.contains("exceeds 256 files"));
    }

    #[cfg(unix)]
    #[test]
    fn portable_symlinked_folder_requires_another_declared_root() {
        let temp = tempfile::tempdir().unwrap();
        let links = temp.path().join("links");
        let pinned = temp.path().join("pinned");
        let target = write_portable_skill(&pinned, "linked", "Pinned linked skill");
        std::fs::create_dir_all(&links).unwrap();
        std::os::unix::fs::symlink(&target, links.join("linked")).unwrap();

        let rejected = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &links,
                "links",
            )],
            &[],
        );
        assert!(rejected.instances().next().is_none());

        let accepted = SkillRegistry::discover_portable(
            vec![
                SkillDiscoveryRoot::new(SkillSourceScope::Workspace, links, "links"),
                SkillDiscoveryRoot::new(SkillSourceScope::User, pinned, "pinned"),
            ],
            &[],
        );
        assert_eq!(accepted.instances().count(), 2);
    }

    fn manage_request(
        action: SkillManageAction,
        selector: String,
        digest: String,
    ) -> SkillManageRequest {
        SkillManageRequest {
            action,
            selector,
            expected_full_tree_digest: digest,
            scope: None,
            relative_path: None,
            content: None,
            old_text: None,
            new_text: None,
            files: Vec::new(),
        }
    }

    #[test]
    fn skill_manage_supports_the_complete_digest_pinned_tree_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        let mut registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                &root,
                "workspace",
            )],
            &[],
        );
        let mut create = manage_request(
            SkillManageAction::Create,
            "repair-workflow".into(),
            "absent".into(),
        );
        create.scope = Some(SkillSourceScope::Workspace);
        create.files = vec![
            SkillManageFile {
                path: "SKILL.md".into(),
                content: Some(
                    "---\nname: repair-workflow\ndescription: Repair a recurring workflow\n---\nRun the documented checks and stop when healthy.\n"
                        .into(),
                ),
            },
            SkillManageFile {
                path: "scripts/check.sh".into(),
                content: Some("#!/bin/sh\nprintf 'healthy\\n'\n".into()),
            },
            SkillManageFile {
                path: "templates/report.txt".into(),
                content: Some("status: pending\n".into()),
            },
            SkillManageFile {
                path: "agents/openai.yaml".into(),
                content: Some(
                    "dependencies:\n  tools:\n    - type: binary\n      value: rsync\n      description: Required at invocation time\n"
                        .into(),
                ),
            },
        ];
        let created = registry.skill_manage(create).unwrap();
        let mut selector = created.instance_id.unwrap();
        let mut digest = created.full_tree_digest.unwrap();
        assert!(root.join("repair-workflow/scripts/check.sh").is_file());
        assert!(root.join("repair-workflow/templates/report.txt").is_file());

        let mut write = manage_request(SkillManageAction::WriteFile, selector, digest);
        write.relative_path = Some("references/guide.md".into());
        write.content = Some("First inspect, then repair, then verify.\n".into());
        let receipt = registry.skill_manage(write).unwrap();
        selector = receipt.instance_id.unwrap();
        digest = receipt.full_tree_digest.unwrap();

        let mut edit = manage_request(SkillManageAction::Edit, selector, digest);
        edit.relative_path = Some("scripts/check.sh".into());
        edit.old_text = Some("healthy".into());
        edit.new_text = Some("verified".into());
        let receipt = registry.skill_manage(edit).unwrap();
        selector = receipt.instance_id.unwrap();
        digest = receipt.full_tree_digest.unwrap();

        let mut patch = manage_request(SkillManageAction::Patch, selector, digest);
        patch.files = vec![SkillManageFile {
            path: "assets/example.txt".into(),
            content: Some("example\n".into()),
        }];
        let receipt = registry.skill_manage(patch).unwrap();
        selector = receipt.instance_id.unwrap();
        digest = receipt.full_tree_digest.unwrap();

        let mut remove = manage_request(SkillManageAction::RemoveFile, selector, digest);
        remove.relative_path = Some("templates/report.txt".into());
        let receipt = registry.skill_manage(remove).unwrap();
        selector = receipt.instance_id.unwrap();
        digest = receipt.full_tree_digest.unwrap();
        assert!(!root.join("repair-workflow/templates/report.txt").exists());

        let delete = manage_request(SkillManageAction::Delete, selector.clone(), digest.clone());
        let receipt = registry.skill_manage(delete).unwrap();
        assert_eq!(receipt.instance_id, Some(selector));
        assert_eq!(receipt.full_tree_digest.as_deref(), Some(digest.as_str()));
        assert!(registry.instances().next().is_none());
        assert!(!root.join("repair-workflow").exists());
    }

    #[test]
    fn skill_manage_rejects_digest_drift_and_managed_skills() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("managed");
        write_portable_skill(&root, "managed-skill", "Managed skill");
        let mut registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Managed,
                root,
                "managed",
            )],
            &[],
        );
        let instance = registry.instances().next().unwrap().clone();
        let mut drifted = manage_request(
            SkillManageAction::WriteFile,
            instance.instance_id.clone(),
            "0".repeat(64),
        );
        drifted.relative_path = Some("references/new.md".into());
        drifted.content = Some("new\n".into());
        assert!(matches!(
            registry.skill_manage(drifted),
            Err(SkillError::Changed(_))
        ));
        let mut managed = manage_request(
            SkillManageAction::WriteFile,
            instance.instance_id,
            instance.content_digest,
        );
        managed.relative_path = Some("references/new.md".into());
        managed.content = Some("new\n".into());
        assert!(matches!(
            registry.skill_manage(managed),
            Err(SkillError::Invalid(message)) if message.contains("read-only")
        ));
    }

    #[test]
    fn prepared_skill_journal_rolls_back_on_registry_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        write_portable_skill(&root, "recoverable", "Recoverable skill");
        let discovery = vec![SkillDiscoveryRoot::new(
            SkillSourceScope::Workspace,
            &root,
            "workspace",
        )];
        let mut registry = SkillRegistry::discover_portable(discovery.clone(), &[]);
        let before = registry.instances().next().unwrap().clone();
        let mut request = manage_request(
            SkillManageAction::WriteFile,
            before.instance_id.clone(),
            before.content_digest.clone(),
        );
        request.relative_path = Some("references/recovery.md".into());
        request.content = Some("new content\n".into());
        let receipt = registry.skill_manage(request).unwrap();
        assert!(root.join("recoverable/references/recovery.md").is_file());
        let journal = temp
            .path()
            .join(".ca-skill-journal")
            .join(receipt.journal_id.to_string())
            .join("journal.json");
        let mut value: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        value["phase"] = Value::String("prepared".into());
        fs::write(&journal, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let recovered = SkillRegistry::discover_portable(discovery, &[]);
        assert!(!root.join("recoverable/references/recovery.md").exists());
        assert_eq!(
            recovered.instances().next().unwrap().content_digest,
            before.content_digest
        );
    }

    #[test]
    fn same_turn_refinement_is_pinned_bounded_and_commits_once() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        write_portable_skill(&root, "invoked", "Invoked skill");
        write_portable_skill(&root, "unrelated", "Unrelated skill");
        let mut registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                root,
                "workspace",
            )],
            &[],
        );
        let mut loads = SkillLoadSet::default();
        registry.load_for_task("invoked", true, &mut loads).unwrap();
        registry.start_refinement_turn(&loads, true);
        let unrelated = registry
            .instances()
            .find(|instance| instance.metadata.name == "unrelated")
            .unwrap()
            .clone();
        let mut rejected = manage_request(
            SkillManageAction::WriteFile,
            unrelated.instance_id,
            unrelated.content_digest,
        );
        rejected.relative_path = Some("references/new.md".into());
        rejected.content = Some("new\n".into());
        assert!(matches!(
            registry.skill_manage(rejected),
            Err(SkillError::Invalid(message)) if message.contains("pinned for this task")
        ));

        let invoked = registry
            .instances()
            .find(|instance| instance.metadata.name == "invoked")
            .unwrap()
            .clone();
        let mut accepted = manage_request(
            SkillManageAction::WriteFile,
            invoked.instance_id,
            invoked.content_digest,
        );
        accepted.relative_path = Some("references/refined.md".into());
        accepted.content = Some("recovered procedure\n".into());
        let receipt = registry.skill_manage(accepted).unwrap();
        let mut second = manage_request(
            SkillManageAction::WriteFile,
            receipt.instance_id.unwrap(),
            receipt.full_tree_digest.unwrap(),
        );
        second.relative_path = Some("references/second.md".into());
        second.content = Some("second mutation\n".into());
        assert!(registry.skill_manage(second).is_err());
    }

    #[test]
    fn hermes_conditions_and_non_secret_config_are_typed_and_precedence_bound() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills/conditional");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!(
                "---\nname: conditional\ndescription: Conditional\nplatforms: [{}]\nmetadata:\n  hermes:\n    requires_tools: [read_file]\n    requires_toolsets: [filesystemread]\n    config:\n      root_path:\n        description: Root path\n        required: true\n      format:\n        default: compact\n---\nUse the configured values as data.\n",
                std::env::consts::OS
            ),
        )
        .unwrap();
        let mut registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                temp.path().join("skills"),
                "test",
            )],
            &[],
        );
        let tools = BTreeSet::from(["read_file".into()]);
        let toolsets = BTreeSet::from(["filesystemread".into()]);
        registry.evaluate_availability(&tools, &toolsets, &BTreeMap::new(), &BTreeMap::new());
        let instance = registry.inspect_instance("conditional").unwrap();
        assert_eq!(instance.availability.setup_required, ["root_path"]);
        assert!(registry
            .portable_catalog_prompt(None)
            .contains("conditional"));
        assert!(matches!(
            registry.resolve_instance("conditional", true),
            Err(SkillError::ConfigurationRequired(_))
        ));

        let profile = BTreeMap::from([(
            "conditional.root_path".into(),
            SkillSettingValue::String("profile".into()),
        )]);
        let workspace = BTreeMap::from([(
            "conditional.root_path".into(),
            SkillSettingValue::String("workspace".into()),
        )]);
        registry.evaluate_availability(&tools, &toolsets, &profile, &workspace);
        let mut loads = SkillLoadSet::default();
        registry
            .load_for_task("conditional", true, &mut loads)
            .unwrap();
        let prompt = registry.task_prompt(&loads).unwrap();
        assert!(prompt.contains("root_path=\"workspace\""));
        assert!(prompt.contains("format=\"compact\""));

        registry.evaluate_availability(&BTreeSet::new(), &toolsets, &profile, &workspace);
        assert!(registry
            .inspect_instance("conditional")
            .unwrap()
            .availability
            .unavailable_reasons
            .iter()
            .any(|reason| reason.contains("missing tools")));
        assert!(!registry
            .portable_catalog_prompt(None)
            .contains("conditional"));

        let secret = "---\nname: secret-config\ndescription: bad\nmetadata:\n  hermes:\n    config:\n      api_token:\n        required: true\n---\nNo.\n";
        assert!(matches!(
            parse_portable_skill(secret),
            Err(SkillError::Invalid(error)) if error.contains("credential-like")
        ));
    }

    #[test]
    fn fallback_toolset_environment_and_qualified_config_conditions_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills/hermes-fallback-test");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: hermes-fallback-test\ndescription: Fallback\nmetadata:\n  hermes:\n    fallback_for_tools: [browser]\n    fallback_for_toolsets: [web]\n    required_environment_variables: [CA_LRN_179_TEST_ENV_THAT_DOES_NOT_EXIST]\n    config:\n      mode:\n        required: true\n---\nUse fallback.\n",
        )
        .unwrap();
        let mut registry = SkillRegistry::discover_portable(
            vec![SkillDiscoveryRoot::new(
                SkillSourceScope::Workspace,
                temp.path().join("skills"),
                "test",
            )],
            &[],
        );
        let instance = registry
            .inspect_instance("hermes-fallback-test")
            .unwrap_or_else(|error| panic!("{error}; diagnostics={:?}", registry.diagnostics()))
            .clone();
        let profile = BTreeMap::from([
            (
                "hermes-fallback-test.mode".into(),
                SkillSettingValue::String("legacy-profile".into()),
            ),
            (
                format!("{}.mode", instance.qualified_name),
                SkillSettingValue::String("exact-profile".into()),
            ),
        ]);
        let workspace = BTreeMap::from([(
            format!("{}.mode", instance.qualified_name),
            SkillSettingValue::String("exact-workspace".into()),
        )]);
        registry.evaluate_availability(
            &BTreeSet::from(["browser".into()]),
            &BTreeSet::from(["web".into()]),
            &profile,
            &workspace,
        );
        let evaluated = registry.inspect_instance("hermes-fallback-test").unwrap();
        assert_eq!(
            evaluated.availability.resolved_config["mode"],
            SkillSettingValue::String("exact-workspace".into())
        );
        assert!(evaluated
            .availability
            .unavailable_reasons
            .iter()
            .any(|reason| reason.contains("fallback is not needed")));
        assert!(evaluated
            .availability
            .unavailable_reasons
            .iter()
            .any(|reason| reason.contains("missing required environment")));
        assert!(!registry
            .portable_catalog_prompt(None)
            .contains("hermes-fallback-test"));
    }

    #[test]
    fn secret_like_values_are_rejected_in_scalars_and_lists() {
        for value in [
            SkillSettingValue::String("sk-not-a-real-secret".into()),
            SkillSettingValue::Strings(vec!["safe".into(), "bearer hidden".into()]),
        ] {
            assert!(validate_setting_value_safety(&value).is_err());
        }
        assert!(
            validate_setting_value_safety(&SkillSettingValue::Strings(vec![
                "/bounded/non-secret/path".into(),
                "compact".into(),
            ]))
            .is_ok()
        );
    }

    #[test]
    fn workspace_skill_config_is_owner_only_typed_and_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profile/skills/workspaces/demo.json");
        save_workspace_skill_config(
            &path,
            "workspace:test:demo.root_path",
            SkillSettingValue::String("/safe/path".into()),
        )
        .unwrap();
        assert_eq!(
            load_workspace_skill_config(&path).unwrap()["workspace:test:demo.root_path"],
            SkillSettingValue::String("/safe/path".into())
        );
        assert!(save_workspace_skill_config(
            &path,
            "workspace:test:demo.api_token",
            SkillSettingValue::String("safe-looking".into()),
        )
        .is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(load_workspace_skill_config(&path).is_err());
        }
    }
}
