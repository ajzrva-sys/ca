use crate::agent::client::ToolDef;
use crate::agent::registry::{ToolOrigin, ToolProfile, ToolRegistry};
use std::collections::{HashMap, HashSet};

const MAX_TOOL_NAME_BYTES: usize = 128;
pub const TRANSPARENT_MECHANISTIC_TOOL_NAMES: [&str; 8] = [
    "git_status",
    "git_diff",
    "git_log",
    "inspect_symbol",
    "inspect_feature",
    "find_relevant_tests",
    "verify_changes",
    "verify_recipe",
];
pub const REMOTE_MECHANISTIC_TOOL_NAMES: [&str; 11] = [
    "list_remote_automation",
    "compare_remote_files",
    "plan_remote_sync",
    "apply_remote_sync",
    "query_remote_rpc",
    "compare_remote_rpc",
    "list_automation_workflows",
    "preview_automation_workflow",
    "correct_automation_workflow",
    "run_automation_workflow",
    "disable_automation_workflow",
];
pub const AUTOMATION_SETUP_TOOL_NAMES: [&str; 3] = [
    "list_automation_templates",
    "preview_automation_setup",
    "apply_automation_setup",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolSelectionPolicy {
    replacement: Option<Vec<String>>,
    include: Vec<String>,
    exclude: Vec<String>,
    discovery_mode: crate::config::ToolDiscoveryMode,
}

impl ToolSelectionPolicy {
    pub fn from_cli(
        replacement: Option<Vec<String>>,
        include: Vec<String>,
        exclude: Vec<String>,
    ) -> Result<Self, ToolSelectionError> {
        Ok(Self {
            replacement: replacement.map(normalize_cli_values).transpose()?,
            include: normalize_cli_values(include)?,
            exclude: normalize_cli_values(exclude)?,
            discovery_mode: crate::config::ToolDiscoveryMode::Eager,
        })
    }

    pub fn replacing(names: &[&str]) -> Self {
        Self::from_cli(
            Some(names.iter().map(|name| (*name).to_string()).collect()),
            Vec::new(),
            Vec::new(),
        )
        .expect("valid test tool names")
    }

    pub fn is_default(&self) -> bool {
        self.replacement.is_none() && self.include.is_empty() && self.exclude.is_empty()
    }

    pub fn with_discovery_mode(mut self, mode: crate::config::ToolDiscoveryMode) -> Self {
        self.discovery_mode = mode;
        self
    }

    pub fn discovery_mode(&self) -> crate::config::ToolDiscoveryMode {
        self.discovery_mode
    }

    pub fn has_explicit_replacement(&self) -> bool {
        self.replacement.is_some()
    }

    pub fn pinned_names(&self) -> impl Iterator<Item = &str> {
        self.replacement
            .iter()
            .flatten()
            .chain(&self.include)
            .map(String::as_str)
    }

    pub fn excluding(mut self, name: &str) -> Self {
        let folded = name.to_ascii_lowercase();
        if !self.exclude.contains(&folded) {
            self.exclude.push(folded);
        }
        self
    }

    fn explicitly_enables_tools(&self) -> bool {
        self.replacement
            .as_ref()
            .is_some_and(|names| !names.is_empty())
            || !self.include.is_empty()
    }

    fn explicitly_enables(&self, wanted: &str) -> bool {
        self.replacement
            .iter()
            .flatten()
            .chain(&self.include)
            .any(|name| name == wanted)
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.replacement
            .iter()
            .flatten()
            .chain(&self.include)
            .chain(&self.exclude)
            .map(String::as_str)
    }
}

fn normalize_cli_values(values: Vec<String>) -> Result<Vec<String>, ToolSelectionError> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        for name in value.split(',') {
            let name = name.trim();
            if name.is_empty()
                || name.len() > MAX_TOOL_NAME_BYTES
                || !name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
            {
                return Err(ToolSelectionError::InvalidName(bounded_name(name)));
            }
            let folded = name.to_ascii_lowercase();
            if seen.insert(folded.clone()) {
                normalized.push(folded);
            }
        }
    }
    Ok(normalized)
}

fn bind_agent_grant_schema(schema: &mut serde_json::Value, grants: &[String]) {
    if let Some(items) = schema.pointer_mut("/properties/tools/items") {
        items["enum"] = serde_json::json!(grants);
    }
    match schema {
        serde_json::Value::Object(fields) => {
            for value in fields.values_mut() {
                bind_agent_grant_schema(value, grants);
            }
        }
        serde_json::Value::Array(items) => {
            for value in items {
                bind_agent_grant_schema(value, grants);
            }
        }
        _ => {}
    }
}

fn bounded_name(name: &str) -> String {
    if name.is_empty() {
        "<empty>".to_string()
    } else if name.len() > MAX_TOOL_NAME_BYTES {
        format!("{}…", name.chars().take(64).collect::<String>())
    } else {
        name.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSelectionConstraints {
    pub tools_supported: bool,
    pub images_supported: bool,
    pub skills_enabled: bool,
    /// `None` retains static compatibility behavior. A runtime-provided empty
    /// list removes the tool; a populated list also constrains its `id` schema.
    pub skill_activation_ids: Option<Vec<String>>,
    pub skill_view_ids: Option<Vec<String>>,
    pub mcp_enabled: bool,
    pub mechanistic_delegation_enabled: bool,
    pub native_mechanistic_enabled: bool,
    pub transparent_mechanistic_enabled: bool,
    pub remote_mechanistic_enabled: bool,
    pub automation_setup_enabled: bool,
    pub lsp_enabled: bool,
    pub memory_generation_enabled: bool,
    pub automation_proposals_enabled: bool,
    pub email_enabled: bool,
    pub allowed_names: Option<HashSet<String>>,
}

impl Default for ToolSelectionConstraints {
    fn default() -> Self {
        Self {
            tools_supported: true,
            images_supported: false,
            skills_enabled: true,
            skill_activation_ids: None,
            skill_view_ids: None,
            mcp_enabled: true,
            mechanistic_delegation_enabled: false,
            native_mechanistic_enabled: false,
            transparent_mechanistic_enabled: false,
            remote_mechanistic_enabled: false,
            automation_setup_enabled: false,
            lsp_enabled: true,
            memory_generation_enabled: false,
            automation_proposals_enabled: false,
            email_enabled: false,
            allowed_names: None,
        }
    }
}

impl ToolSelectionConstraints {
    pub fn restricted_to<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.allowed_names = Some(
            names
                .into_iter()
                .map(|name| name.as_ref().to_ascii_lowercase())
                .collect(),
        );
        self
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ToolSelectionError {
    #[error("invalid tool name: {0}")]
    InvalidName(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool names differ only by case: {0}")]
    AmbiguousName(String),
    #[error("selected model does not support tools")]
    ToolsUnsupported,
    #[error("selected model does not support image input")]
    ImagesUnsupported,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedToolSet {
    definitions: Vec<ToolDef>,
    built_in: Vec<ToolDef>,
    mcp: Vec<ToolDef>,
}

impl ResolvedToolSet {
    pub fn definitions(&self) -> &[ToolDef] {
        &self.definitions
    }

    pub fn into_definitions(self) -> Vec<ToolDef> {
        self.definitions
    }

    pub fn built_in_definitions(&self) -> &[ToolDef] {
        &self.built_in
    }

    pub fn mcp_definitions(&self) -> &[ToolDef] {
        &self.mcp
    }
}

pub fn resolve_tools(
    registry: &ToolRegistry,
    profile: ToolProfile,
    policy: &ToolSelectionPolicy,
    constraints: &ToolSelectionConstraints,
) -> Result<ResolvedToolSet, ToolSelectionError> {
    resolve(
        registry,
        profile,
        policy,
        constraints,
        UnknownNamePolicy::Strict,
    )
}

pub fn resolve_internal_tools(
    registry: &ToolRegistry,
    profile: ToolProfile,
    policy: &ToolSelectionPolicy,
    constraints: &ToolSelectionConstraints,
) -> Result<ResolvedToolSet, ToolSelectionError> {
    resolve(
        registry,
        profile,
        policy,
        constraints,
        UnknownNamePolicy::IgnoreDynamicMcp,
    )
}

pub fn resolve_preview_tools(
    registry: &ToolRegistry,
    profile: ToolProfile,
    policy: &ToolSelectionPolicy,
    constraints: &ToolSelectionConstraints,
) -> Result<ResolvedToolSet, ToolSelectionError> {
    resolve(
        registry,
        profile,
        policy,
        constraints,
        UnknownNamePolicy::IgnoreAll,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnknownNamePolicy {
    Strict,
    IgnoreDynamicMcp,
    IgnoreAll,
}

fn resolve(
    registry: &ToolRegistry,
    profile: ToolProfile,
    policy: &ToolSelectionPolicy,
    constraints: &ToolSelectionConstraints,
    unknown_policy: UnknownNamePolicy,
) -> Result<ResolvedToolSet, ToolSelectionError> {
    let mut by_folded = HashMap::<String, &str>::new();
    for spec in registry.specs() {
        if spec.name.as_str().len() > MAX_TOOL_NAME_BYTES {
            return Err(ToolSelectionError::InvalidName(bounded_name(
                spec.name.as_str(),
            )));
        }
        let folded = spec.name.as_str().to_ascii_lowercase();
        if let Some(previous) = by_folded.insert(folded, spec.name.as_str()) {
            if previous != spec.name.as_str() {
                return Err(ToolSelectionError::AmbiguousName(
                    spec.name.as_str().to_string(),
                ));
            }
        }
    }

    for name in policy.names() {
        if by_folded.contains_key(name) {
            continue;
        }
        let ignored = match unknown_policy {
            UnknownNamePolicy::Strict => false,
            UnknownNamePolicy::IgnoreDynamicMcp => name.starts_with("mcp__"),
            UnknownNamePolicy::IgnoreAll => true,
        };
        if !ignored {
            return Err(ToolSelectionError::UnknownTool(name.to_string()));
        }
    }

    if !constraints.tools_supported {
        if policy.explicitly_enables_tools() {
            return Err(ToolSelectionError::ToolsUnsupported);
        }
        return Ok(ResolvedToolSet::default());
    }
    if !constraints.images_supported
        && policy
            .replacement
            .iter()
            .flatten()
            .chain(&policy.include)
            .any(|name| name == "view_image")
    {
        return Err(ToolSelectionError::ImagesUnsupported);
    }

    let eligible = registry
        .specs()
        .iter()
        .filter(|spec| spec.profiles.contains(&profile))
        .map(|spec| spec.name.as_str().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut selected = match &policy.replacement {
        Some(names) => canonical_eligible(names, &by_folded, &eligible),
        None => eligible.clone(),
    };
    selected.extend(canonical_eligible(&policy.include, &by_folded, &eligible));
    for name in canonical_eligible(&policy.exclude, &by_folded, &eligible) {
        selected.remove(&name);
    }
    if policy.discovery_mode == crate::config::ToolDiscoveryMode::Eager
        && !policy
            .pinned_names()
            .any(|name| name == crate::tool_discovery::SEARCH_TOOLS_NAME)
    {
        selected.remove(crate::tool_discovery::SEARCH_TOOLS_NAME);
    }

    if !constraints.skills_enabled {
        selected.remove("activate_skill");
        selected.remove("skill_view");
        selected.remove("skill_manage");
    } else {
        if constraints
            .skill_activation_ids
            .as_ref()
            .is_some_and(Vec::is_empty)
        {
            selected.remove("activate_skill");
        }
        if constraints
            .skill_view_ids
            .as_ref()
            .is_some_and(Vec::is_empty)
        {
            selected.remove("skill_view");
        }
    }
    if !constraints.mcp_enabled {
        selected.remove("search_mcp_tools");
        selected.retain(|name| !name.starts_with("mcp__"));
    }
    if !constraints.mcp_enabled || !constraints.mechanistic_delegation_enabled {
        selected.remove("delegate_mechanistic");
        for name in TRANSPARENT_MECHANISTIC_TOOL_NAMES {
            selected.remove(name);
        }
        for name in REMOTE_MECHANISTIC_TOOL_NAMES {
            selected.remove(name);
        }
        for name in AUTOMATION_SETUP_TOOL_NAMES {
            selected.remove(name);
        }
    } else if constraints.transparent_mechanistic_enabled {
        if !policy.explicitly_enables("delegate_mechanistic") {
            selected.remove("delegate_mechanistic");
        }
        if !constraints.remote_mechanistic_enabled {
            for name in REMOTE_MECHANISTIC_TOOL_NAMES {
                selected.remove(name);
            }
        }
        if !constraints.automation_setup_enabled {
            for name in AUTOMATION_SETUP_TOOL_NAMES {
                selected.remove(name);
            }
        }
    } else {
        for name in TRANSPARENT_MECHANISTIC_TOOL_NAMES {
            selected.remove(name);
        }
        for name in REMOTE_MECHANISTIC_TOOL_NAMES {
            selected.remove(name);
        }
        if !constraints.automation_setup_enabled {
            for name in AUTOMATION_SETUP_TOOL_NAMES {
                selected.remove(name);
            }
        }
    }
    if constraints.native_mechanistic_enabled {
        for name in TRANSPARENT_MECHANISTIC_TOOL_NAMES {
            if eligible.contains(name)
                && !policy.exclude.iter().any(|excluded| excluded == name)
                && policy
                    .replacement
                    .as_ref()
                    .is_none_or(|names| names.iter().any(|selected| selected == name))
            {
                selected.insert(name.into());
            }
        }
    }
    if !constraints.lsp_enabled {
        selected.retain(|name| !name.starts_with("lsp_"));
    }
    if !constraints.images_supported {
        selected.remove("view_image");
    }
    if !constraints.memory_generation_enabled {
        selected.remove("propose_memory");
    }
    if !constraints.automation_proposals_enabled {
        selected.remove("propose_automation");
        selected.remove("list_local_automations");
    }
    for name in crate::email::EMAIL_TOOL_NAMES {
        if !constraints.email_enabled || !policy.explicitly_enables(name) {
            selected.remove(name);
        }
    }
    if !policy.explicitly_enables("browser") {
        selected.remove("browser");
    }
    if let Some(allowed) = &constraints.allowed_names {
        selected.retain(|name| allowed.contains(name));
    }

    let mut definitions = Vec::new();
    let mut built_in = Vec::new();
    let mut mcp = Vec::new();
    for spec in registry.specs() {
        let folded = spec.name.as_str().to_ascii_lowercase();
        if !spec.profiles.contains(&profile) || !selected.contains(&folded) {
            continue;
        }
        let mut definition = spec.definition();
        let skill_ids = match spec.name.as_str() {
            "activate_skill" => constraints.skill_activation_ids.as_deref(),
            "skill_view" => constraints.skill_view_ids.as_deref(),
            _ => None,
        };
        if let Some(ids) = skill_ids {
            let field = if spec.name.as_str() == "skill_view" {
                "instance_id"
            } else {
                "id"
            };
            if let Some(id_schema) = definition
                .function
                .parameters
                .get_mut("properties")
                .and_then(|properties| properties.get_mut(field))
            {
                id_schema["enum"] = serde_json::Value::Array(
                    ids.iter().cloned().map(serde_json::Value::String).collect(),
                );
            }
        }
        if matches!(spec.name.as_str(), "spawn_agent" | "spawn_agent_batch") {
            let grants = registry
                .specs()
                .iter()
                .filter(|candidate| {
                    selected.contains(&candidate.name.as_str().to_ascii_lowercase())
                        && candidate.profiles.contains(&ToolProfile::Subagent)
                        && !candidate
                            .capabilities
                            .contains(&crate::agent::registry::ToolCapability::Subagent)
                        && !matches!(
                            candidate.name.as_str(),
                            "propose_plan"
                                | "configure_agent_graph"
                                | "activate_skill"
                                | "propose_memory"
                        )
                })
                .map(|candidate| candidate.name.as_str().to_string())
                .collect::<Vec<_>>();
            bind_agent_grant_schema(&mut definition.function.parameters, &grants);
        }
        definitions.push(definition.clone());
        match &spec.origin {
            ToolOrigin::BuiltIn => built_in.push(definition),
            ToolOrigin::Mcp(_) => mcp.push(definition),
        }
    }
    Ok(ResolvedToolSet {
        definitions,
        built_in,
        mcp,
    })
}

fn canonical_eligible(
    names: &[String],
    registry_names: &HashMap<String, &str>,
    eligible: &HashSet<String>,
) -> HashSet<String> {
    names
        .iter()
        .filter(|name| registry_names.contains_key(name.as_str()) && eligible.contains(*name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_repository_tools_and_agent_grants_match_effective_catalog() {
        let policy = ToolSelectionPolicy::replacing(&["spawn_agent", "read_file", "git_status"]);
        let constraints = ToolSelectionConstraints {
            native_mechanistic_enabled: true,
            ..Default::default()
        };
        let resolved = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &constraints,
        )
        .unwrap();
        assert!(resolved
            .definitions()
            .iter()
            .any(|tool| tool.function.name == "git_status"));
        let spawn = resolved
            .definitions()
            .iter()
            .find(|tool| tool.function.name == "spawn_agent")
            .unwrap();
        let grants = spawn
            .function
            .parameters
            .pointer("/properties/tools/items/enum")
            .unwrap()
            .as_array()
            .unwrap();
        assert!(grants.iter().any(|name| name == "read_file"));
        assert!(!grants
            .iter()
            .any(|name| name == "spawn_agent" || name == "write_file"));
        let disabled = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!disabled
            .definitions()
            .iter()
            .any(|tool| tool.function.name == "git_status"));
    }

    use super::*;

    fn names(set: &ResolvedToolSet) -> Vec<&str> {
        set.definitions()
            .iter()
            .map(|definition| definition.function.name.as_str())
            .collect()
    }

    #[test]
    fn agent_route_discovery_obeys_parent_catalog_and_is_not_a_child_grant() {
        let registry = ToolRegistry::builtins();
        let constraints = ToolSelectionConstraints::default();
        let policy =
            ToolSelectionPolicy::replacing(&["spawn_agent", "list_agent_routes", "read_file"]);
        let main = resolve_tools(registry, ToolProfile::Main, &policy, &constraints).unwrap();
        assert!(names(&main).contains(&"list_agent_routes"));
        let spawn = main
            .definitions()
            .iter()
            .find(|tool| tool.function.name == "spawn_agent")
            .unwrap();
        let grants = spawn
            .function
            .parameters
            .pointer("/properties/tools/items/enum")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(grants, &[serde_json::json!("read_file")]);
        let excluded_policy =
            ToolSelectionPolicy::from_cli(None, Vec::new(), vec!["list_agent_routes".into()])
                .unwrap();
        let excluded =
            resolve_tools(registry, ToolProfile::Main, &excluded_policy, &constraints).unwrap();
        assert!(!names(&excluded).contains(&"list_agent_routes"));
        for profile in [
            ToolProfile::Subagent,
            ToolProfile::Plan,
            ToolProfile::Review,
            ToolProfile::Checkpoint,
        ] {
            let restricted = resolve_tools(registry, profile, &policy, &constraints).unwrap();
            assert_eq!(
                names(&restricted).contains(&"list_agent_routes"),
                profile == ToolProfile::Plan
            );
        }
    }

    #[test]
    fn eager_schema_defaults_preserve_preview_parity_and_explicit_search_pins() {
        let registry = ToolRegistry::builtins();
        let constraints = ToolSelectionConstraints::default();
        let eager = resolve_tools(
            registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &constraints,
        )
        .unwrap();
        let preview = resolve_preview_tools(
            registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &constraints,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(eager.definitions()).unwrap(),
            serde_json::to_value(preview.definitions()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(eager.definitions()).unwrap(),
            serde_json::to_value(crate::agent::tools::default_tool_defs()).unwrap()
        );
        assert!(!names(&eager).contains(&crate::tool_discovery::SEARCH_TOOLS_NAME));
        let lazy = resolve_tools(
            registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::default()
                .with_discovery_mode(crate::config::ToolDiscoveryMode::Lazy),
            &constraints,
        )
        .unwrap();
        assert!(names(&lazy).contains(&crate::tool_discovery::SEARCH_TOOLS_NAME));
        for policy in [
            ToolSelectionPolicy::from_cli(
                None,
                vec![crate::tool_discovery::SEARCH_TOOLS_NAME.into()],
                Vec::new(),
            )
            .unwrap(),
            ToolSelectionPolicy::replacing(&[crate::tool_discovery::SEARCH_TOOLS_NAME]),
        ] {
            let pinned = resolve_tools(registry, ToolProfile::Main, &policy, &constraints).unwrap();
            assert!(names(&pinned).contains(&crate::tool_discovery::SEARCH_TOOLS_NAME));
        }
        for profile in [ToolProfile::Review, ToolProfile::Checkpoint] {
            let eager = resolve_internal_tools(
                registry,
                profile,
                &ToolSelectionPolicy::default(),
                &constraints,
            )
            .unwrap();
            let lazy = resolve_internal_tools(
                registry,
                profile,
                &ToolSelectionPolicy::default()
                    .with_discovery_mode(crate::config::ToolDiscoveryMode::Lazy),
                &constraints,
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(eager.definitions()).unwrap(),
                serde_json::to_value(lazy.definitions()).unwrap()
            );
        }
    }

    #[test]
    fn replacement_then_include_then_exclude_preserves_registry_order() {
        let policy = ToolSelectionPolicy::from_cli(
            Some(vec!["WRITE_FILE,READ_FILE".into()]),
            vec!["search_files".into(), "read_file".into()],
            vec!["write_file".into()],
        )
        .unwrap();
        let set = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(names(&set), ["read_file", "search_files"]);
    }

    #[test]
    fn profile_is_an_upper_bound_and_checkpoint_stays_empty() {
        let policy = ToolSelectionPolicy::replacing(&["write_file", "read_file"]);
        let review = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Review,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(names(&review), ["read_file"]);
        let checkpoint = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Checkpoint,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(checkpoint.definitions().is_empty());

        let plan = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Plan,
            &ToolSelectionPolicy::replacing(&["write_file", "propose_plan"]),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(names(&plan), ["propose_plan"]);
    }

    #[test]
    fn restrictions_only_remove_tools() {
        let policy = ToolSelectionPolicy::default();
        let constraints = ToolSelectionConstraints {
            skills_enabled: false,
            ..ToolSelectionConstraints::default()
        }
        .restricted_to(["read_file", "activate_skill", "write_file"]);
        let set = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &constraints,
        )
        .unwrap();
        assert_eq!(names(&set), ["read_file", "write_file"]);
    }

    #[test]
    fn mechanistic_facade_is_opt_in_main_only_and_requires_mcp() {
        let registry = ToolRegistry::builtins();
        let policy = ToolSelectionPolicy::default();
        let default = resolve_tools(
            registry,
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!names(&default).contains(&"delegate_mechanistic"));

        let enabled = ToolSelectionConstraints {
            mechanistic_delegation_enabled: true,
            ..ToolSelectionConstraints::default()
        };
        let main = resolve_tools(registry, ToolProfile::Main, &policy, &enabled).unwrap();
        assert!(names(&main).contains(&"delegate_mechanistic"));
        let delegation = main
            .definitions()
            .iter()
            .find(|definition| definition.function.name == "delegate_mechanistic")
            .unwrap();
        assert!(delegation
            .function
            .description
            .contains("Automatically prefer this"));
        assert!(delegation
            .function
            .description
            .contains("The user does not need to request delegation"));
        assert!(delegation
            .function
            .description
            .contains("fall back only when it explicitly allows fallback"));
        assert!(delegation
            .function
            .description
            .contains("already pinned to its repository"));
        assert!(delegation
            .function
            .description
            .contains("match_mode is regex"));
        assert_eq!(
            delegation.function.parameters["required"],
            serde_json::json!(["actions"])
        );
        assert!(delegation.function.parameters["properties"]
            .get("schema")
            .is_none());
        let plan = resolve_tools(registry, ToolProfile::Plan, &policy, &enabled).unwrap();
        let review = resolve_tools(registry, ToolProfile::Review, &policy, &enabled).unwrap();
        let checkpoint =
            resolve_tools(registry, ToolProfile::Checkpoint, &policy, &enabled).unwrap();
        let subagent = resolve_tools(registry, ToolProfile::Subagent, &policy, &enabled).unwrap();
        for set in [&plan, &review, &checkpoint, &subagent] {
            assert!(!names(set).contains(&"delegate_mechanistic"));
        }

        let no_mcp = ToolSelectionConstraints {
            mcp_enabled: false,
            mechanistic_delegation_enabled: true,
            ..ToolSelectionConstraints::default()
        };
        let main = resolve_tools(registry, ToolProfile::Main, &policy, &no_mcp).unwrap();
        assert!(!names(&main).contains(&"delegate_mechanistic"));
    }

    #[test]
    fn transparent_routing_hides_facade_by_default_and_exposes_virtual_tools_main_only() {
        let enabled = ToolSelectionConstraints {
            mechanistic_delegation_enabled: true,
            transparent_mechanistic_enabled: true,
            ..ToolSelectionConstraints::default()
        };
        let main = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &enabled,
        )
        .unwrap();
        let main_names = names(&main);
        assert!(!main_names.contains(&"delegate_mechanistic"));
        for name in TRANSPARENT_MECHANISTIC_TOOL_NAMES {
            assert!(main_names.contains(&name));
        }
        for name in REMOTE_MECHANISTIC_TOOL_NAMES {
            assert!(!main_names.contains(&name));
        }

        let remote_enabled = ToolSelectionConstraints {
            mechanistic_delegation_enabled: true,
            transparent_mechanistic_enabled: true,
            remote_mechanistic_enabled: true,
            ..ToolSelectionConstraints::default()
        };
        let remote_main = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &remote_enabled,
        )
        .unwrap();
        for name in REMOTE_MECHANISTIC_TOOL_NAMES {
            assert!(names(&remote_main).contains(&name));
        }
        let plan = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Plan,
            &ToolSelectionPolicy::default(),
            &remote_enabled,
        )
        .unwrap();
        for name in REMOTE_MECHANISTIC_TOOL_NAMES {
            assert!(!names(&plan).contains(&name));
        }
        let apply = ToolRegistry::builtins().get("apply_remote_sync").unwrap();
        assert_eq!(
            apply.mutation,
            crate::agent::registry::ToolMutation::Mutating
        );
        assert!(apply
            .capabilities
            .contains(&crate::agent::registry::ToolCapability::Network));

        let explicit = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::from_cli(None, vec!["delegate_mechanistic".into()], Vec::new())
                .unwrap(),
            &enabled,
        )
        .unwrap();
        assert!(names(&explicit).contains(&"delegate_mechanistic"));
        for profile in [
            ToolProfile::Plan,
            ToolProfile::Review,
            ToolProfile::Checkpoint,
            ToolProfile::Subagent,
        ] {
            let set = resolve_tools(
                ToolRegistry::builtins(),
                profile,
                &ToolSelectionPolicy::default(),
                &enabled,
            )
            .unwrap();
            for name in TRANSPARENT_MECHANISTIC_TOOL_NAMES {
                assert!(!names(&set).contains(&name));
            }
        }
    }

    #[test]
    fn automation_setup_is_available_before_remote_activation_and_main_only() {
        let enabled = ToolSelectionConstraints {
            mechanistic_delegation_enabled: true,
            automation_setup_enabled: true,
            ..ToolSelectionConstraints::default()
        };
        let main = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &enabled,
        )
        .unwrap();
        for name in AUTOMATION_SETUP_TOOL_NAMES {
            assert!(names(&main).contains(&name));
        }
        for profile in [
            ToolProfile::Plan,
            ToolProfile::Review,
            ToolProfile::Checkpoint,
            ToolProfile::Subagent,
        ] {
            let tools = resolve_tools(
                ToolRegistry::builtins(),
                profile,
                &ToolSelectionPolicy::default(),
                &enabled,
            )
            .unwrap();
            for name in AUTOMATION_SETUP_TOOL_NAMES {
                assert!(!names(&tools).contains(&name));
            }
        }
        let apply = ToolRegistry::builtins()
            .get("apply_automation_setup")
            .unwrap();
        assert_eq!(
            apply.mutation,
            crate::agent::registry::ToolMutation::Mutating
        );
        assert!(apply
            .capabilities
            .contains(&crate::agent::registry::ToolCapability::FilesystemWrite));
        assert!(!apply
            .capabilities
            .contains(&crate::agent::registry::ToolCapability::Network));
    }

    #[test]
    fn runtime_skill_availability_omits_impossible_tools_and_constrains_ids() {
        let empty = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                skill_activation_ids: Some(Vec::new()),
                skill_view_ids: Some(Vec::new()),
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(!names(&empty).contains(&"activate_skill"));
        assert!(!names(&empty).contains(&"skill_view"));

        let available = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                skill_activation_ids: Some(vec!["workspace:review".into()]),
                skill_view_ids: Some(Vec::new()),
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        let activation = available
            .definitions()
            .iter()
            .find(|definition| definition.function.name == "activate_skill")
            .unwrap();
        assert_eq!(
            activation.function.parameters["properties"]["id"]["enum"],
            serde_json::json!(["workspace:review"])
        );
        assert!(!names(&available).contains(&"skill_view"));
    }

    #[test]
    fn email_tools_require_config_and_exact_explicit_selection() {
        let default = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                email_enabled: true,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(!names(&default)
            .iter()
            .any(|name| name.starts_with("email_")));

        let policy = ToolSelectionPolicy::replacing(&["email_read", "email_send"]);
        let disabled = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(names(&disabled).is_empty());

        let enabled = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints {
                email_enabled: true,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert_eq!(names(&enabled), ["email_read", "email_send"]);

        for profile in [
            ToolProfile::Plan,
            ToolProfile::Review,
            ToolProfile::Subagent,
            ToolProfile::Checkpoint,
        ] {
            let selected = resolve_tools(
                ToolRegistry::builtins(),
                profile,
                &policy,
                &ToolSelectionConstraints {
                    email_enabled: true,
                    ..ToolSelectionConstraints::default()
                },
            )
            .unwrap();
            assert!(selected.definitions().is_empty(), "{profile:?}");
        }
    }

    #[test]
    fn unknown_names_fail_only_at_final_resolution() {
        let policy = ToolSelectionPolicy::replacing(&["mcp__future__tool"]);
        assert!(resolve_preview_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap()
        .definitions()
        .is_empty());
        assert_eq!(
            resolve_tools(
                ToolRegistry::builtins(),
                ToolProfile::Main,
                &policy,
                &ToolSelectionConstraints::default(),
            )
            .unwrap_err(),
            ToolSelectionError::UnknownTool("mcp__future__tool".into())
        );
        let internal = resolve_internal_tools(
            ToolRegistry::builtins(),
            ToolProfile::Review,
            &policy,
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(internal.definitions().is_empty());
        assert!(matches!(
            resolve_internal_tools(
                ToolRegistry::builtins(),
                ToolProfile::Review,
                &ToolSelectionPolicy::replacing(&["ordinary_typo"]),
                &ToolSelectionConstraints::default(),
            ),
            Err(ToolSelectionError::UnknownTool(name)) if name == "ordinary_typo"
        ));
    }

    #[test]
    fn explicit_selection_rejects_a_toolless_model() {
        let error = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::replacing(&["read_file"]),
            &ToolSelectionConstraints {
                tools_supported: false,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap_err();
        assert_eq!(error, ToolSelectionError::ToolsUnsupported);

        let default = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                tools_supported: false,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(default.definitions().is_empty());
    }

    #[test]
    fn dynamic_mcp_tools_obey_the_same_policy_and_keep_their_origin() {
        let mut specs = ToolRegistry::builtins().specs().to_vec();
        specs.push(
            crate::agent::registry::ToolSpec::mcp(
                "mcp__demo__echo",
                "demo",
                "Echo".into(),
                serde_json::json!({"type":"object"}),
            )
            .unwrap(),
        );
        let registry = ToolRegistry::new(specs).unwrap();

        let restrictive = resolve_tools(
            &registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::replacing(&["read_file"]),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(names(&restrictive), ["read_file"]);
        assert!(restrictive.mcp_definitions().is_empty());

        let included = resolve_tools(
            &registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::from_cli(
                Some(vec!["read_file".into()]),
                vec!["MCP__DEMO__ECHO".into()],
                Vec::new(),
            )
            .unwrap(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert_eq!(names(&included), ["read_file", "mcp__demo__echo"]);
        assert_eq!(included.built_in_definitions().len(), 1);
        assert_eq!(included.mcp_definitions().len(), 1);
    }

    #[test]
    fn disabled_mcp_capability_removes_search_and_dynamic_schemas() {
        let mut specs = ToolRegistry::builtins().specs().to_vec();
        specs.push(
            crate::agent::registry::ToolSpec::mcp(
                "mcp__demo__echo",
                "demo",
                "echo".into(),
                serde_json::json!({"type":"object"}),
            )
            .unwrap(),
        );
        let registry = ToolRegistry::new(specs).unwrap();
        let enabled = resolve_tools(
            &registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(names(&enabled).contains(&"search_mcp_tools"));
        assert!(names(&enabled).contains(&"mcp__demo__echo"));

        let disabled = resolve_tools(
            &registry,
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                mcp_enabled: false,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(!names(&disabled).contains(&"search_mcp_tools"));
        assert!(!names(&disabled).contains(&"mcp__demo__echo"));
    }

    #[test]
    fn disabled_lsp_capability_removes_every_semantic_navigation_schema() {
        let enabled = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(names(&enabled).contains(&"lsp_symbols"));
        assert!(names(&enabled).contains(&"lsp_call_hierarchy"));

        let disabled = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                lsp_enabled: false,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(!names(&disabled).iter().any(|name| name.starts_with("lsp_")));
    }

    #[test]
    fn image_tool_requires_an_image_capable_model() {
        let hidden = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!names(&hidden).contains(&"view_image"));

        let visible = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                images_supported: true,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(names(&visible).contains(&"view_image"));

        assert_eq!(
            resolve_tools(
                ToolRegistry::builtins(),
                ToolProfile::Main,
                &ToolSelectionPolicy::replacing(&["view_image"]),
                &ToolSelectionConstraints::default(),
            )
            .unwrap_err(),
            ToolSelectionError::ImagesUnsupported
        );
    }

    #[test]
    fn memory_proposal_tool_requires_explicit_generation_enablement() {
        let hidden = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!names(&hidden).contains(&"propose_memory"));

        let visible = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints {
                memory_generation_enabled: true,
                ..ToolSelectionConstraints::default()
            },
        )
        .unwrap();
        assert!(names(&visible).contains(&"propose_memory"));
    }

    #[test]
    fn browser_requires_explicit_selection_and_exclusion_still_wins() {
        let hidden = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::default(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!names(&hidden).contains(&"browser"));

        let included = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::from_cli(None, vec!["Browser".into()], Vec::new()).unwrap(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(names(&included).contains(&"browser"));

        let excluded = resolve_tools(
            ToolRegistry::builtins(),
            ToolProfile::Main,
            &ToolSelectionPolicy::from_cli(None, vec!["browser".into()], vec!["browser".into()])
                .unwrap(),
            &ToolSelectionConstraints::default(),
        )
        .unwrap();
        assert!(!names(&excluded).contains(&"browser"));
    }

    #[test]
    fn invalid_empty_and_matcher_values_are_rejected() {
        assert!(matches!(
            ToolSelectionPolicy::from_cli(Some(vec!["read_file,".into()]), vec![], vec![]),
            Err(ToolSelectionError::InvalidName(_))
        ));
        assert!(matches!(
            ToolSelectionPolicy::from_cli(Some(vec!["run_command(cargo)".into()]), vec![], vec![]),
            Err(ToolSelectionError::InvalidName(_))
        ));
        let oversized = "x".repeat(MAX_TOOL_NAME_BYTES + 1000);
        let error = ToolSelectionPolicy::from_cli(Some(vec![oversized]), vec![], vec![])
            .unwrap_err()
            .to_string();
        assert!(error.len() < 200);
    }
}
#[test]
fn automation_proposals_require_a_host_preview_surface_and_profile() {
    fn names(set: &ResolvedToolSet) -> Vec<&str> {
        set.definitions()
            .iter()
            .map(|tool| tool.function.name.as_str())
            .collect()
    }
    let registry = ToolRegistry::builtins();
    let policy = ToolSelectionPolicy::default();
    let constraints = ToolSelectionConstraints::default();
    assert!(
        !names(&resolve_tools(registry, ToolProfile::Main, &policy, &constraints).unwrap())
            .contains(&"propose_automation")
    );
    assert!(
        !names(&resolve_tools(registry, ToolProfile::Main, &policy, &constraints).unwrap())
            .contains(&"list_local_automations")
    );
    let enabled = ToolSelectionConstraints {
        automation_proposals_enabled: true,
        ..constraints
    };
    assert!(
        names(&resolve_tools(registry, ToolProfile::Main, &policy, &enabled).unwrap())
            .contains(&"propose_automation")
    );
    assert!(
        names(&resolve_tools(registry, ToolProfile::Main, &policy, &enabled).unwrap())
            .contains(&"list_local_automations")
    );
    for profile in [
        ToolProfile::Plan,
        ToolProfile::Subagent,
        ToolProfile::Review,
        ToolProfile::Checkpoint,
    ] {
        assert!(
            !names(&resolve_internal_tools(registry, profile, &policy, &enabled).unwrap())
                .contains(&"propose_automation")
        );
        assert!(
            !names(&resolve_internal_tools(registry, profile, &policy, &enabled).unwrap())
                .contains(&"list_local_automations")
        );
    }
}
