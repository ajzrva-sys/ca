//! Harness component registry — tracks content hashes of all harness components
//! active during a session, enabling change-outcome correlation.
//!
//! Each component is hashed independently so we can detect exactly which
//! component changed between sessions and correlate that with outcome deltas.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::path::Path;

/// Identifies a harness component type. Maps to AHE's 7-component model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessComponent {
    /// AGENTS.md / CLAUDE.md project instructions.
    AgentsMd,
    Build,
    ModelRoute,
    /// The assembled system prompt (may include rules, skills, memory).
    SystemPrompt,
    /// Tool definitions — names, descriptions, schemas.
    ToolDefinitions,
    /// Active skills and their content.
    ActiveSkills,
    /// Approval policy configuration (allowlist/autoreview/yolo + rules).
    ApprovalPolicy,
    /// Memory store contents (user + workspace).
    MemoryStore,
    /// Configuration file (config.toml relevant fields).
    Config,
    /// QUALITY.md guardrails if present.
    QualityGuardrails,
    /// Hooks configuration.
    Hooks,
    /// Sub-agent definitions (.ca/agents/*.md).
    SubAgentDefinitions,
}

/// SHA-256 hex digest.
pub type DigestHex = String;

/// A snapshot of all harness component hashes at a point in time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentSnapshot {
    pub components: BTreeMap<HarnessComponent, DigestHex>,
    /// Timestamp when the snapshot was taken (Unix epoch seconds).
    pub captured_at: u64,
}

impl ComponentSnapshot {
    /// Returns `true` if the given component changed between this snapshot and `other`.
    pub fn component_changed(&self, other: &Self, component: HarnessComponent) -> bool {
        match (
            self.components.get(&component),
            other.components.get(&component),
        ) {
            (Some(a), Some(b)) => a != b,
            (None, None) => false,
            _ => true,
        }
    }

    /// Returns all components that differ between two snapshots.
    pub fn diff(&self, other: &Self) -> Vec<HarnessComponent> {
        let all_keys: std::collections::HashSet<_> = self
            .components
            .keys()
            .chain(other.components.keys())
            .cloned()
            .collect();
        all_keys
            .into_iter()
            .filter(|k| self.component_changed(other, *k))
            .collect()
    }
}

/// Capture a snapshot of the harness components from the filesystem.
///
/// `project_root` is the workspace root containing AGENTS.md, .ca/, etc.
/// `config_path` is the path to config.toml.
pub fn capture_snapshot(project_root: &Path, config_path: Option<&Path>) -> ComponentSnapshot {
    let mut components = BTreeMap::new();

    // AGENTS.md / CLAUDE.md
    for name in &["AGENTS.md", "CLAUDE.md"] {
        let path = project_root.join(name);
        if let Ok(hash) = hash_file(&path) {
            components.insert(HarnessComponent::AgentsMd, hash);
            break;
        }
    }

    // QUALITY.md
    let quality_path = project_root.join("QUALITY.md");
    if let Ok(hash) = hash_file(&quality_path) {
        components.insert(HarnessComponent::QualityGuardrails, hash);
    }

    // Skills — hash the skills directory contents
    let skills_dir = project_root.join(".ca/skills");
    if let Ok(hash) = hash_dir_sorted(&skills_dir) {
        components.insert(HarnessComponent::ActiveSkills, hash);
    }

    // Sub-agent definitions
    let agents_dir = project_root.join(".ca/agents");
    if let Ok(hash) = hash_dir_sorted(&agents_dir) {
        components.insert(HarnessComponent::SubAgentDefinitions, hash);
    }

    // Effective configuration is captured from prepared requests, never from a
    // config file that can contain credentials. This is only a disk inventory.
    let _ = config_path;

    ComponentSnapshot {
        components,
        captured_at: now_epoch(),
    }
}

/// Identity of effective runtime inputs, independent of conversation contents.
pub fn capture_prepared(request: &crate::request::PreparedModelRequest) -> ComponentSnapshot {
    let mut components = request.harness_components.clone();
    let instructions: Vec<_> = request
        .messages
        .iter()
        .filter(|m| matches!(m.role.as_str(), "system" | "developer"))
        .map(|m| &m.content)
        .collect();
    let mut tools = request.tools.clone();
    tools.sort_by(|a, b| a.function.name.cmp(&b.function.name));
    components.insert(
        HarnessComponent::SystemPrompt,
        hex_sha256(&serde_json::to_vec(&instructions).unwrap_or_default()),
    );
    components.insert(
        HarnessComponent::ToolDefinitions,
        hex_sha256(&serde_json::to_vec(&tools).unwrap_or_default()),
    );
    components.insert(
        HarnessComponent::ModelRoute,
        match &request.runtime_source {
            Some(source) => hex_sha256(
                &serde_json::to_vec(&(source, &request.provider, &request.model, &request.options))
                    .unwrap_or_default(),
            ),
            None => hex_sha256(
                format!(
                    "{}\0{}\0{:?}",
                    request.provider, request.model, request.options
                )
                .as_bytes(),
            ),
        },
    );
    components.insert(HarnessComponent::Build, env!("CA_BUILD_DIGEST").into());
    ComponentSnapshot {
        components,
        captured_at: now_epoch(),
    }
}

/// Include the actual protocol and normalized endpoint identity without storing
/// the endpoint or credentials in outcome records.
pub fn capture_prepared_on_route(
    request: &crate::request::PreparedModelRequest,
    source: &crate::agent::stream::ProviderStateSourceV1,
) -> ComponentSnapshot {
    let mut snapshot = capture_prepared(request);
    snapshot.components.insert(
        HarnessComponent::ModelRoute,
        hex_sha256(
            &serde_json::to_vec(&(source, &request.provider, &request.model, &request.options))
                .unwrap_or_default(),
        ),
    );
    snapshot
}

impl ComponentSnapshot {
    pub fn digest(&self) -> String {
        hex_sha256(&serde_json::to_vec(&self.components).unwrap_or_default())
    }
    pub fn outcome_snapshot(&self) -> crate::session_outcome::HarnessSnapshot {
        let get = |key| self.components.get(&key).cloned();
        crate::session_outcome::HarnessSnapshot {
            build: get(HarnessComponent::Build),
            model_route: get(HarnessComponent::ModelRoute),
            agents_md: get(HarnessComponent::AgentsMd),
            system_prompt: get(HarnessComponent::SystemPrompt),
            tool_definitions: get(HarnessComponent::ToolDefinitions),
            active_skills: get(HarnessComponent::ActiveSkills),
            approval_policy: get(HarnessComponent::ApprovalPolicy),
            memory_store: get(HarnessComponent::MemoryStore),
            config: get(HarnessComponent::Config),
            quality_guardrails: get(HarnessComponent::QualityGuardrails),
            hooks: get(HarnessComponent::Hooks),
        }
    }
}

/// Capture just the agents_md hash from a project root.
#[allow(dead_code)]
pub fn hash_agents_md(project_root: &Path) -> Option<DigestHex> {
    for name in &["AGENTS.md", "CLAUDE.md"] {
        let path = project_root.join(name);
        if let Ok(hash) = hash_file(&path) {
            return Some(hash);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_file(path: &Path) -> std::io::Result<DigestHex> {
    let data = std::fs::read(path)?;
    Ok(hex_sha256(&data))
}

/// Hash all files in a directory (sorted by name for determinism).
fn hash_dir_sorted(dir: &Path) -> std::io::Result<DigestHex> {
    if !dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("directory not found: {}", dir.display()),
        ));
    }
    let mut entries: Vec<_> = walk_files(dir)?
        .into_iter()
        .filter_map(|p| std::fs::read(&p).ok().map(|data| (p, data)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = sha2::Sha256::new();
    for (path, data) in &entries {
        hasher.update(
            path.strip_prefix(dir)
                .unwrap_or(path)
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update(data);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn walk_files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut result = Vec::new();
    if !dir.is_dir() {
        return Ok(result);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            result.extend(walk_files(&path)?);
        } else {
            result.push(path);
        }
    }
    Ok(result)
}

fn hex_sha256(data: &[u8]) -> String {
    let hash = sha2::Sha256::digest(data);
    format!("{hash:x}")
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Snapshots are write-once per digest and only the newest is ever read back
/// (`load_latest_snapshot`). Retaining a small recent window keeps diff history
/// without letting the directory grow one file per harness configuration ever seen.
pub const MAX_RETAINED_SNAPSHOTS: usize = 8;

/// Save a component snapshot to disk, pruning older snapshots beyond the
/// retention window.
pub fn save_snapshot(snapshot: &ComponentSnapshot, data_dir: &Path) -> std::io::Result<()> {
    let dir = data_dir.join("harness-snapshots");
    std::fs::create_dir_all(&dir)?;
    let filename = format!("{}.json", snapshot.digest());
    let path = dir.join(filename);
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut temporary = tempfile::NamedTempFile::new_in(&dir)?;
    use std::io::Write;
    temporary.write_all(json.as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(path) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.error),
    }
    prune_snapshots(&dir)
}

/// List snapshot files newest-first by mtime.
fn snapshot_files_newest_first(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            (modified, e.path())
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(entries.into_iter().map(|(_, path)| path).collect())
}

/// Drop snapshots older than the retention window. Best-effort: a locked or
/// unreadable leftover never blocks saving the current snapshot.
fn prune_snapshots(dir: &Path) -> std::io::Result<()> {
    let files = snapshot_files_newest_first(dir)?;
    for path in files.into_iter().skip(MAX_RETAINED_SNAPSHOTS) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Load the most recent snapshot from disk.
pub fn load_latest_snapshot(data_dir: &Path) -> std::io::Result<Option<ComponentSnapshot>> {
    let dir = data_dir.join("harness-snapshots");
    if !dir.exists() {
        return Ok(None);
    }
    let files = snapshot_files_newest_first(&dir)?;
    if let Some(path) = files.first() {
        let data = std::fs::read_to_string(path)?;
        let snapshot: ComponentSnapshot = serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        return Ok(Some(snapshot));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// CLI helpers
// ---------------------------------------------------------------------------

/// Format a snapshot as a human-readable table.
pub fn format_snapshot(snapshot: &ComponentSnapshot) -> String {
    if snapshot.components.is_empty() {
        return "No harness components captured.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{:<24} {}\n", "Component", "Hash (first 16)"));
    out.push_str(&"-".repeat(48));
    out.push('\n');
    for (component, hash) in &snapshot.components {
        let short: String = hash.chars().take(16).collect();
        out.push_str(&format!("{:<24} {}\n", format!("{:?}", component), short));
    }
    out
}

/// Format a diff between two snapshots.
#[allow(dead_code)]
pub fn format_diff(before: &ComponentSnapshot, after: &ComponentSnapshot) -> String {
    let changed = before.diff(after);
    if changed.is_empty() {
        return "No harness components changed.".into();
    }
    let mut out = String::new();
    out.push_str("Changed components:\n");
    for component in &changed {
        let old_hash = before
            .components
            .get(component)
            .map(|h| h.chars().take(12).collect::<String>())
            .unwrap_or_else(|| "none".into());
        let new_hash = after
            .components
            .get(component)
            .map(|h| h.chars().take(12).collect::<String>())
            .unwrap_or_else(|| "none".into());
        out.push_str(&format!(
            "  {:?}: {} -> {}\n",
            component, old_hash, new_hash
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_diff_detects_changes() {
        let mut before = ComponentSnapshot::default();
        before
            .components
            .insert(HarnessComponent::AgentsMd, "abc123".into());
        before
            .components
            .insert(HarnessComponent::Config, "def456".into());

        let mut after = ComponentSnapshot::default();
        after
            .components
            .insert(HarnessComponent::AgentsMd, "abc123".into());
        after
            .components
            .insert(HarnessComponent::Config, "changed".into());

        let diff = before.diff(&after);
        assert_eq!(diff.len(), 1);
        assert!(diff.contains(&HarnessComponent::Config));
        assert!(!diff.contains(&HarnessComponent::AgentsMd));
    }

    #[test]
    fn component_changed_detects_missing() {
        let mut before = ComponentSnapshot::default();
        before
            .components
            .insert(HarnessComponent::Hooks, "aaa".into());

        let after = ComponentSnapshot::default();
        assert!(before.component_changed(&after, HarnessComponent::Hooks));
    }

    #[test]
    fn save_snapshot_prunes_to_the_retention_window() {
        let dir = tempfile::tempdir().unwrap();
        let total = MAX_RETAINED_SNAPSHOTS + 5;
        for index in 0..total {
            let mut snapshot = ComponentSnapshot::default();
            // Distinct component hashes give distinct digests, so each save is
            // a new file rather than an idempotent rewrite.
            snapshot
                .components
                .insert(HarnessComponent::Config, format!("digest-{index}"));
            snapshot.captured_at = index as u64 + 1;
            save_snapshot(&snapshot, dir.path()).unwrap();
            // Pin mtimes so newest-first ordering is deterministic.
            let path = dir
                .path()
                .join("harness-snapshots")
                .join(format!("{}.json", snapshot.digest()));
            let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + index as u64);
            file.set_modified(when).unwrap();
        }
        let mut stored: Vec<_> = std::fs::read_dir(dir.path().join("harness-snapshots"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        stored.sort();
        assert_eq!(stored.len(), MAX_RETAINED_SNAPSHOTS);

        // The newest survives and still loads.
        let loaded = load_latest_snapshot(dir.path()).unwrap().unwrap();
        assert_eq!(
            loaded.components.get(&HarnessComponent::Config).unwrap(),
            &format!("digest-{}", total - 1)
        );
    }

    #[test]
    fn load_latest_snapshot_survives_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_latest_snapshot(dir.path()).unwrap().is_none());
    }
}

#[cfg(test)]
mod runtime_identity_tests {
    use super::*;
    #[test]
    fn hrs253_runtime_identity_excludes_conversation_and_time() {
        let message = |role: &str, text: &str| crate::agent::client::ChatMessage {
            role: role.into(),
            content: crate::image_content::ModelContent::text(text),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        };
        let mut request = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::MainTurn,
            "provider",
            "model",
            vec![
                message("system", "effective instructions"),
                message("user", "private conversation"),
            ],
            Vec::new(),
            Default::default(),
        );
        let source = crate::agent::stream::ProviderStateSourceV1::from_route(
            "provider",
            crate::agent::provider::ProviderProtocol::ChatCompletions,
            "https://first.example/v1",
        )
        .unwrap();
        let first_route = capture_prepared_on_route(&request, &source);
        let second_source = crate::agent::stream::ProviderStateSourceV1::from_route(
            "provider",
            crate::agent::provider::ProviderProtocol::ChatCompletions,
            "https://second.example/v1",
        )
        .unwrap();
        assert_ne!(
            first_route.digest(),
            capture_prepared_on_route(&request, &second_source).digest()
        );
        let mut other_protocol = source.clone();
        other_protocol.protocol = crate::agent::provider::ProviderProtocol::OpenAiResponses;
        assert_ne!(
            first_route.digest(),
            capture_prepared_on_route(&request, &other_protocol).digest()
        );
        assert!(!serde_json::to_string(&first_route)
            .unwrap()
            .contains("first.example"));
        let first = capture_prepared(&request);
        request.messages[1] = message("user", "different private conversation");
        let mut second = capture_prepared(&request);
        second.captured_at += 100;
        assert_eq!(first.digest(), second.digest());
        assert!(first.outcome_snapshot().build.is_some());
        request.model = "different model".into();
        assert_ne!(first.digest(), capture_prepared(&request).digest());
        request.messages[0] = message("system", "changed instructions");
        assert_ne!(
            first.components[&HarnessComponent::SystemPrompt],
            capture_prepared(&request).components[&HarnessComponent::SystemPrompt]
        );
        assert!(!serde_json::to_string(&first)
            .unwrap()
            .contains("private conversation"));
    }
}
