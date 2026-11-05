//! Host-issued observations used only to project evidence into a provider request.
//!
//! Saved metadata describes an observation; it never grants access or proves that
//! disk is still unchanged. Only this process's issuance registry supplies live
//! provenance. The canonical transcript and provider replay items stay untouched.
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use uuid::Uuid;

const VERSION: u32 = 1;
const MAX_ISSUED: usize = 4096;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    Source,
    Directory,
    Search,
    Mutation,
    Execution,
    PresentationOnly,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Omission {
    DuplicateEarlier(Uuid),
    DuplicateLater(Uuid),
    HistoricalSource,
    Age,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Note {
    SavedUnknown,
    HistoricalCommand,
    UnknownSource,
    PriorSourcesSuperseded,
    PriorVerificationInvalidated,
    PriorEffectsUnknown,
    IsolatedCommand,
    CopyArtifact,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CommandCopyEvidence {
    generation_sha256: String,
    root_sha256: String,
    original_subject_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Presentation {
    binding_sha256: String,
    omission: Option<Omission>,
    notes: Vec<Note>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PresentationCell {
    value: Option<Presentation>,
    // Never persisted. A saved rendering cannot authenticate host evidence.
    local: bool,
    host: bool,
}

#[derive(Debug, Default)]
struct PresentationState(Mutex<PresentationCell>);

impl PartialEq for PresentationState {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        let left = self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .value
            .clone();
        let right = other
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .value
            .clone();
        left == right
    }
}
impl Eq for PresentationState {}

fn empty_presentation(value: &Arc<PresentationState>) -> bool {
    value
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .value
        .is_none()
}
fn serialize_presentation<S: serde::Serializer>(
    value: &Arc<PresentationState>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .value
        .serialize(serializer)
}
fn deserialize_presentation<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<PresentationState>, D::Error> {
    let value = Option::<Presentation>::deserialize(deserializer)?;
    Ok(Arc::new(PresentationState(Mutex::new(PresentationCell {
        value,
        local: false,
        host: false,
    }))))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObservationV1 {
    version: u32,
    nonce: Uuid,
    pub kind: ObservationKind,
    workspace_sha256: String,
    pub subjects_sha256: Vec<String>,
    pub selection_sha256: String,
    pub result_sha256: String,
    pub source_sha256: Option<String>,
    tool_sha256: String,
    binding_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command_copy: Option<CommandCopyEvidence>,
    #[serde(
        default,
        skip_serializing_if = "empty_presentation",
        serialize_with = "serialize_presentation",
        deserialize_with = "deserialize_presentation"
    )]
    presentation: Arc<PresentationState>,
}

#[derive(Default)]
struct Issued {
    order: VecDeque<Uuid>,
    bindings: HashMap<Uuid, String>,
    observations: HashMap<Uuid, ObservationV1>,
    read_sources: HashMap<String, String>,
}

fn issued() -> &'static Mutex<Issued> {
    static ISSUED: OnceLock<Mutex<Issued>> = OnceLock::new();
    ISSUED.get_or_init(|| Mutex::new(Issued::default()))
}

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn hash_fields(fields: &[&str]) -> String {
    digest(serde_json::to_vec(fields).expect("string fields serialize"))
}

fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn kind(name: &str, success: bool) -> Option<ObservationKind> {
    match name {
        "run_command" | "await_job" | "run_privileged" | "write_shell_stdin" => {
            Some(ObservationKind::Execution)
        }
        _ if !success => None,
        "read_file" => Some(ObservationKind::Source),
        "list_dir" => Some(ObservationKind::Directory),
        "search_files" | "glob" => Some(ObservationKind::Search),
        "write_file" | "edit_file" | "hashline_edit" | "apply_patch" | "delete_file"
        | "move_files" => Some(ObservationKind::Mutation),
        "git_status" | "git_diff" | "git_log" | "lsp_definition" | "lsp_references"
        | "lsp_hover" | "lsp_implementation" | "lsp_call_hierarchy" => {
            Some(ObservationKind::PresentationOnly)
        }
        _ => None,
    }
}

/// Call at a completed native tool boundary, with canonical host identities.
/// `source_sha256` must describe bytes read by that operation, never a later read.
pub fn capture_host(
    name: &str,
    arguments: &str,
    output: &str,
    success: bool,
    workspace_identity: &str,
    canonical_subjects: &[String],
    source_sha256: Option<&str>,
) -> Option<ObservationV1> {
    let kind = kind(name, success)?;
    let workspace_sha256 = digest(workspace_identity);
    let mut subjects_sha256 = canonical_subjects
        .iter()
        .take(256)
        .map(|path| hash_fields(&[&workspace_sha256, path]))
        .collect::<Vec<_>>();
    subjects_sha256.sort();
    subjects_sha256.dedup();
    let observation = ObservationV1 {
        version: VERSION,
        nonce: Uuid::new_v4(),
        kind,
        workspace_sha256,
        subjects_sha256,
        selection_sha256: hash_fields(&[name, arguments]),
        result_sha256: digest(output),
        source_sha256: source_sha256
            .filter(|value| is_digest(value))
            .map(str::to_owned),
        tool_sha256: digest(name),
        binding_sha256: hash_fields(&[name, arguments, output]),
        command_copy: None,
        presentation: Arc::default(),
    };
    register(&observation);
    Some(observation)
}

fn register(observation: &ObservationV1) {
    let mut registry = issued().lock().unwrap_or_else(|poison| poison.into_inner());
    let binding = observation.issuance_identity();
    if registry
        .bindings
        .insert(observation.nonce, binding)
        .is_none()
    {
        registry.order.push_back(observation.nonce);
    }
    registry
        .observations
        .insert(observation.nonce, observation.clone());
    while registry.order.len() > MAX_ISSUED {
        if let Some(nonce) = registry.order.pop_front() {
            registry.bindings.remove(&nonce);
            registry.observations.remove(&nonce);
        }
    }
}

impl ObservationV1 {
    fn issuance_identity(&self) -> String {
        let mut base = self.clone();
        base.presentation = Arc::default();
        digest(serde_json::to_vec(&base).expect("observation serializes"))
    }

    fn saved_presentation(&self) -> PresentationCell {
        self.presentation
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn presentation_valid(&self) -> bool {
        self.saved_presentation()
            .value
            .as_ref()
            .is_none_or(|value| {
                value.binding_sha256 == self.issuance_identity()
                    && value.notes.len() <= 8
                    && value
                        .notes
                        .iter()
                        .enumerate()
                        .all(|(i, note)| !value.notes[..i].contains(note))
                    && match value.omission {
                        Some(
                            Omission::DuplicateEarlier(basis) | Omission::DuplicateLater(basis),
                        ) => basis != self.nonce,
                        _ => true,
                    }
            })
    }

    pub fn matches(&self, name: &str, arguments: &str, output: &str) -> bool {
        self.version == VERSION
            && self.presentation_valid()
            && self.subjects_sha256.len() <= 256
            && is_digest(&self.workspace_sha256)
            && self.subjects_sha256.iter().all(|value| is_digest(value))
            && self.source_sha256.as_deref().is_none_or(is_digest)
            && self.command_copy.as_ref().is_none_or(|scope| {
                is_digest(&scope.generation_sha256)
                    && is_digest(&scope.root_sha256)
                    && is_digest(&scope.original_subject_sha256)
            })
            && is_digest(&self.selection_sha256)
            && self.tool_sha256 == digest(name)
            && self.result_sha256 == digest(output)
            && self.binding_sha256 == hash_fields(&[name, arguments, output])
    }

    pub fn live(&self) -> bool {
        let registry = issued().lock().unwrap_or_else(|poison| poison.into_inner());
        registry.bindings.get(&self.nonce).is_some_and(|binding| {
            self.presentation_valid() && *binding == self.issuance_identity()
        })
    }

    pub fn same_workspace(&self, other: &Self) -> bool {
        self.workspace_sha256 == other.workspace_sha256
    }

    fn same_effect_scope(&self, other: &Self) -> bool {
        self.same_workspace(other)
            && self
                .command_copy
                .as_ref()
                .map(|scope| &scope.generation_sha256)
                == other
                    .command_copy
                    .as_ref()
                    .map(|scope| &scope.generation_sha256)
    }

    /// A restored source may regain live provenance only after an authorized
    /// asynchronous read has matched the original whole-source digest.
    pub fn revalidate_source(&self, observed_source_sha256: &str) -> bool {
        if self.kind != ObservationKind::Source
            || self.command_copy.is_some()
            || !self.presentation_valid()
            || self.source_sha256.as_deref() != Some(observed_source_sha256)
            || !is_digest(observed_source_sha256)
        {
            return false;
        }
        register(self);
        true
    }
}

/// Convenience for already-prepared native arguments. This performs no reads.
#[cfg(test)]
pub fn capture_native_execution(
    name: &str,
    arguments: &serde_json::Value,
    output: &str,
    success: bool,
    workspace: &crate::workspace::WorkspaceContext,
) -> Option<ObservationV1> {
    capture_native_execution_scoped(name, arguments, output, success, workspace, None, None)
}

pub(crate) fn capture_native_execution_scoped(
    name: &str,
    arguments: &serde_json::Value,
    output: &str,
    success: bool,
    workspace: &crate::workspace::WorkspaceContext,
    command_cwd: Option<&str>,
    privacy: Option<&crate::privacy::Context>,
) -> Option<ObservationV1> {
    let cwd = workspace.cwd();
    let subjects = arguments
        .get("path")
        .and_then(serde_json::Value::as_str)
        .or_else(|| matches!(name, "list_dir" | "search_files" | "glob").then_some("."))
        .map(|path| {
            let path = std::path::Path::new(path);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            vec![path.to_string_lossy().into_owned()]
        })
        .unwrap_or_default();
    let source = subjects.first().and_then(|path| {
        issued()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .read_sources
            .get(&hash_fields(&[path, output]))
            .cloned()
    });
    let mut observation = capture_host(
        name,
        &arguments.to_string(),
        output,
        success,
        &cwd.to_string_lossy(),
        &subjects,
        source.as_deref(),
    )?;
    let path = if observation.kind == ObservationKind::Execution {
        command_cwd
    } else {
        subjects.first().map(String::as_str)
    };
    if let Some((generation_sha256, root_sha256, original)) =
        path.and_then(|path| privacy?.command_copy_binding(std::path::Path::new(path)))
    {
        observation.command_copy = Some(CommandCopyEvidence {
            generation_sha256,
            root_sha256,
            original_subject_sha256: hash_fields(&[
                &observation.workspace_sha256,
                &original.to_string_lossy(),
            ]),
        });
        register(&observation);
    }
    Some(observation)
}

/// Associate a returned native read with the full source bytes hashed by that
/// very read. Paths and output are retained only as a bounded composite digest.
pub fn remember_read_source(resolved: &str, output: &str, sha256: &str) {
    if !is_digest(sha256) {
        return;
    }
    let mut registry = issued().lock().unwrap_or_else(|poison| poison.into_inner());
    if registry.read_sources.len() >= MAX_ISSUED {
        registry.read_sources.clear();
    }
    registry
        .read_sources
        .insert(hash_fields(&[resolved, output]), sha256.into());
}

/// Preserve host-issued execution identity through a host-owned presentation
/// transform. Source excerpts cannot acquire provenance through this API.
pub fn record_transformed_execution(original: &str, rendered: &str) {
    if original == rendered {
        return;
    }
    let result = digest(original);
    let observations: Vec<_> = issued()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .observations
        .values()
        .filter(|observation| {
            observation.kind == ObservationKind::Execution && observation.result_sha256 == result
        })
        .cloned()
        .collect();
    for mut observation in observations {
        observation.nonce = Uuid::new_v4();
        observation.presentation = Arc::default();
        observation.result_sha256 = digest(rendered);
        observation.binding_sha256 =
            digest([result.as_bytes(), observation.result_sha256.as_bytes()].concat());
        register(&observation);
    }
}

/// Bind native provenance to the model's original spelling only when the
/// completed native result identifies one unambiguous observation.
pub fn bind_native_result(name: &str, arguments: &str, output: &str) -> Option<ObservationV1> {
    let tool = digest(name);
    let result = digest(output);
    let candidates = {
        let registry = issued().lock().unwrap_or_else(|poison| poison.into_inner());
        registry
            .order
            .iter()
            .rev()
            .filter_map(|nonce| registry.observations.get(nonce))
            .filter(|observation| {
                observation.tool_sha256 == tool && observation.result_sha256 == result
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if let Some(exact) = candidates
        .iter()
        .find(|observation| observation.matches(name, arguments, output))
    {
        let mut bound = exact.clone();
        bound.nonce = Uuid::new_v4();
        bound.presentation = Arc::default();
        register(&bound);
        return Some(bound);
    }
    let first = candidates.first()?;
    if candidates.iter().any(|other| {
        other.workspace_sha256 != first.workspace_sha256
            || other.subjects_sha256 != first.subjects_sha256
            || other.selection_sha256 != first.selection_sha256
            || other.command_copy != first.command_copy
    }) {
        return None;
    }
    let mut rebound = first.clone();
    rebound.nonce = Uuid::new_v4();
    rebound.presentation = Arc::default();
    rebound.binding_sha256 = hash_fields(&[name, arguments, output]);
    register(&rebound);
    Some(rebound)
}

/// Revalidate one restored source observation on a blocking worker. The caller
/// may supply native arguments translated through the existing privacy boundary;
/// absent that translation, an unresolved alias remains unknown.
pub async fn revalidate_saved_call(
    call: &crate::app::ToolCall,
    workspace: &crate::workspace::WorkspaceContext,
    native_arguments: Option<serde_json::Value>,
) -> bool {
    let Some(observation) = call.observation.clone() else {
        return false;
    };
    {
        let mut registry = issued().lock().unwrap_or_else(|poison| poison.into_inner());
        registry.bindings.remove(&observation.nonce);
        registry.observations.remove(&observation.nonce);
    }
    let Some(output) = call.output.as_deref() else {
        return false;
    };
    if call.name != "read_file"
        || call.status != crate::app::ToolStatus::Done
        || !observation.matches(&call.name, &call.arguments, output)
        || observation.kind != ObservationKind::Source
        || observation.source_sha256.is_none()
    {
        return false;
    }
    let arguments = native_arguments.or_else(|| serde_json::from_str(&call.arguments).ok());
    let Some(path) = arguments
        .as_ref()
        .and_then(|arguments| arguments.get("path"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        return false;
    };
    let arguments = arguments.expect("path was checked");
    if observation.selection_sha256 != hash_fields(&[&call.name, &arguments.to_string()])
        || observation.subjects_sha256.len() != 1
    {
        return false;
    }
    let start_line = arguments
        .get("start_line")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let end_line = arguments
        .get("end_line")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    let hashes = arguments
        .get("include_line_hashes")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let workspace = workspace.clone();
    let requested = path.clone();
    let checked_observation = observation.clone();
    let resolved = tokio::task::spawn_blocking(move || {
        let cwd = workspace.cwd();
        let declared = std::path::Path::new(&path);
        let declared = if declared.is_absolute() {
            declared.to_path_buf()
        } else {
            cwd.join(declared)
        };
        let identity = digest(cwd.to_string_lossy().as_bytes());
        let subject = hash_fields(&[&identity, &declared.to_string_lossy()]);
        if checked_observation.workspace_sha256 != identity
            || !checked_observation.subjects_sha256.contains(&subject)
        {
            return None;
        }
        let Ok(path) = workspace.resolve(&path, crate::workspace::PathOperation::Read) else {
            return None;
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            return None;
        };
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return None;
        }
        path.to_str().map(str::to_owned)
    })
    .await
    .ok()
    .flatten();
    let Some(resolved) = resolved else {
        return false;
    };
    // Recreate the actual native result, including range/line hashes/footer.
    // Matching only a full-file digest would let invented saved excerpts become
    // trusted observations of an otherwise real file.
    let Ok(rendered) =
        crate::agent::tools::read_file_range(&resolved, &requested, start_line, end_line, hashes)
            .await
    else {
        return false;
    };
    if rendered != output {
        return false;
    }
    let source = issued()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .read_sources
        .get(&hash_fields(&[&resolved, &rendered]))
        .cloned();
    source
        .as_deref()
        .is_some_and(|source| observation.revalidate_source(source))
}

/// New optional metadata must not make an old or imported session unloadable.
pub fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<ObservationV1>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value
        .and_then(|value| serde_json::from_value(value).ok())
        .filter(ObservationV1::presentation_valid))
}

fn affects(change: &ObservationV1, source: &ObservationV1) -> bool {
    change.same_workspace(source)
        && (change.same_effect_scope(source)
            || (change.kind == ObservationKind::Execution && change.command_copy.is_none()))
        && (change.kind == ObservationKind::Execution
            || change.subjects_sha256.is_empty()
            || source.subjects_sha256.is_empty()
            || change
                .subjects_sha256
                .iter()
                .any(|id| source.subjects_sha256.contains(id)))
}
fn equivalent(a: &ObservationV1, b: &ObservationV1) -> bool {
    a.same_effect_scope(b)
        && a.selection_sha256 == b.selection_sha256
        && a.result_sha256 == b.result_sha256
        && a.source_sha256 == b.source_sha256
}
fn source_like(kind: ObservationKind) -> bool {
    matches!(
        kind,
        ObservationKind::Source | ObservationKind::Directory | ObservationKind::Search
    )
}

impl Presentation {
    fn original(observation: &ObservationV1) -> Self {
        Self {
            binding_sha256: observation.issuance_identity(),
            omission: None,
            notes: Vec::new(),
        }
    }
    fn render(
        &self,
        original: &crate::image_content::ModelContent,
        name: &str,
        id: &str,
        basis_id: Option<&str>,
    ) -> crate::image_content::ModelContent {
        use crate::image_content::ModelContent;
        let mut result = match self.omission {
            None => original.clone(),
            Some(Omission::DuplicateEarlier(_) | Omission::DuplicateLater(_)) => {
                let basis = basis_id.unwrap_or("unknown").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(128).collect::<String>();
                ModelContent::context_marker(format!("[CA evidence: duplicate of retained call {basis}; reconfirmed only at this call.]"))
            },
            Some(Omission::HistoricalSource) => ModelContent::context_marker("[CA evidence: historical source observation superseded by a host-observed change. Read the current source; original evidence remains in canonical history.]"),
            Some(Omission::Age) => {
                let safe_id = id.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(128).collect::<String>();
                ModelContent::context_marker(format!("[Earlier result of {name} call {safe_id} omitted from this request. Full output remains in canonical session history; use session_search/session recall, or repeat the read if unavailable.]"))
            }
        };
        for note in &self.notes {
            result = result.append_authored(match note {
                Note::SavedUnknown => "\n[CA evidence: saved observation; current source freshness is unknown until host revalidation.]",
                Note::HistoricalCommand => "\n[CA evidence: historical command result; a later host-observed edit means this result does not verify the current revision.]",
                Note::UnknownSource => "\n[CA evidence: a later command may have changed this source; current freshness is unknown.]",
                Note::PriorSourcesSuperseded => "\n[CA evidence: historical source observations of the affected source describe its revision before this host-observed change. Read the changed source for its current revision. Reuse unaffected evidence when still valid.]",
                Note::PriorVerificationInvalidated => "\n[CA evidence: an earlier command result does not verify the current revision after this host-observed edit. Its original outcome remains historical evidence.]",
                Note::PriorEffectsUnknown => "\n[CA evidence: this command may have changed previously observed sources; their current freshness is unknown until host inspection.]",
                Note::IsolatedCommand => "\n[CA evidence: command ran in an isolated writable copy. It did not change original project files. Reuse retained original reads unless independently invalidated or a fresh read was requested. Inspect only relevant copy files to establish the command effect.]",
                Note::CopyArtifact => "\n[CA evidence: command-copy artifact. Cite this copy only for copy-state claims. Use retained original reads for original claims when still valid. Publishing copy changes requires task intent.]",
            });
        }
        result
    }
}

struct ProjectionSlot {
    index: usize,
    observation: ObservationV1,
    original: crate::image_content::ModelContent,
    name: String,
    id: String,
    proposed: Presentation,
    frozen: bool,
    cacheable: bool,
    commit: bool,
}

fn basis_call<'a>(presentation: &Presentation, slots: &'a [ProjectionSlot]) -> Option<&'a str> {
    let nonce = match presentation.omission {
        Some(Omission::DuplicateEarlier(nonce) | Omission::DuplicateLater(nonce)) => nonce,
        _ => return None,
    };
    slots
        .iter()
        .find(|slot| slot.observation.nonce == nonce)
        .map(|slot| slot.id.as_str())
}

/// Freeze the final semantic AND age-pruning presentation at one boundary.
/// Canonical content and its private-source annotations are never replaced.
pub(crate) struct EvidenceProjection {
    slots: Vec<ProjectionSlot>,
    lookup: HashMap<usize, usize>,
}
impl EvidenceProjection {
    pub(crate) fn frozen(&self, index: usize) -> bool {
        self.lookup
            .get(&index)
            .is_some_and(|slot| self.slots[*slot].frozen)
    }
    pub(crate) fn age(&mut self, index: usize) -> bool {
        let Some(slot_index) = self.lookup.get(&index) else {
            return false;
        };
        let slot = &mut self.slots[*slot_index];
        if slot.frozen {
            return false;
        }
        slot.proposed.omission = Some(Omission::Age);
        true
    }
    pub(crate) fn finish(
        self,
        messages: &mut [crate::agent::client::ChatMessage],
    ) -> crate::context_pruning::PruningStats {
        let mut decisions = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let mut cell = slot
                .observation
                .presentation
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            cell.host |= slot.cacheable;
            let decision = if cell.local {
                cell.value
                    .clone()
                    .expect("local presentation has a decision")
            } else if slot.commit {
                // First local writer wins. Every contender renders that winner.
                cell.value = Some(slot.proposed.clone());
                cell.local = true;
                slot.proposed.clone()
            } else {
                slot.proposed.clone()
            };
            decisions.push(decision);
        }
        let mut stats = crate::context_pruning::PruningStats::default();
        for (slot, decision) in self.slots.iter().zip(&decisions) {
            let mut rendered = decision.clone();
            if let Some(Omission::DuplicateEarlier(basis) | Omission::DuplicateLater(basis)) =
                rendered.omission
            {
                let retained = self.slots.iter().zip(&decisions).any(|(other, view)| {
                    other.observation.nonce == basis
                        && view.omission.is_none()
                        && equivalent(&slot.observation, &other.observation)
                });
                if !retained {
                    // A branch may have removed the basis. Never claim it remains.
                    rendered.omission = None;
                    if !rendered.notes.contains(&Note::SavedUnknown) {
                        rendered.notes.push(Note::SavedUnknown);
                    }
                }
            }
            let content = rendered.render(
                &slot.original,
                &slot.name,
                &slot.id,
                basis_call(&rendered, &self.slots),
            );
            let before =
                crate::context::estimate_text_tokens(slot.original.as_str().unwrap_or_default());
            let after = crate::context::estimate_text_tokens(content.as_str().unwrap_or_default());
            stats.results += usize::from(rendered.omission.is_some());
            stats.saved_tokens += before.saturating_sub(after);
            stats.added_tokens += after.saturating_sub(before);
            messages[slot.index].content = content;
        }
        stats
    }
}

/// Select distinct observations before age pruning, retaining past decisions.
pub(crate) fn project_deferred(
    messages: &mut [crate::agent::client::ChatMessage],
    semantic: bool,
) -> EvidenceProjection {
    let mut calls = HashMap::new();
    let mut invalid = std::collections::HashSet::new();
    let mut results = std::collections::HashSet::new();
    for message in messages.iter() {
        for call in message.tool_calls.iter().flatten() {
            if calls.insert(call.id.clone(), call.clone()).is_some() {
                invalid.insert(call.id.clone());
            }
        }
        if let Some(id) = &message.tool_call_id {
            if !results.insert(id.clone()) || !calls.contains_key(id) {
                invalid.insert(id.clone());
            }
        }
    }
    let protected = messages
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .filter(|batch| {
            batch
                .iter()
                .any(|call| !results.contains(&call.id) || invalid.contains(&call.id))
        })
        .flat_map(|batch| batch.iter().map(|call| call.id.clone()))
        .collect::<std::collections::HashSet<_>>();
    let opaque_history = messages.iter().any(|message| {
        message
            .response_items
            .iter()
            .any(|item| item.opaque_source().is_some())
    });
    let mut slots = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if message.role != "tool" {
            continue;
        }
        let Some(id) = message.tool_call_id.as_deref() else {
            continue;
        };
        if invalid.contains(id) || protected.contains(id) {
            continue;
        }
        let Some(call) = calls.get(id) else {
            continue;
        };
        let Some(text) = message.content.as_str() else {
            continue;
        };
        let Some(observation) =
            message
                .content
                .context_observation()
                .cloned()
                .filter(|observation| {
                    observation.matches(&call.function.name, &call.function.arguments, text)
                })
        else {
            continue;
        };
        let saved = observation.saved_presentation();
        let live = observation.live();
        let mut proposed = Presentation::original(&observation);
        if !live && !saved.local {
            proposed.notes.push(Note::SavedUnknown);
        }
        slots.push(ProjectionSlot {
            index,
            original: message.content.clone(),
            name: call.function.name.clone(),
            id: id.into(),
            proposed: saved
                .value
                .clone()
                .filter(|_| saved.local)
                .unwrap_or(proposed),
            frozen: saved.local,
            cacheable: live || saved.host,
            commit: live || saved.host || !opaque_history,
            observation,
        });
    }
    // A saved choice is descriptive data. Reauthenticate its constant conditions
    // from this actual history, rather than treating a successful read as proof
    // of an arbitrary saved omission or warning.
    for i in 0..slots.len() {
        if slots[i].frozen || !slots[i].cacheable {
            continue;
        }
        let Some(saved) = slots[i].observation.saved_presentation().value else {
            continue;
        };
        let current = &slots[i];
        let omission_valid = match saved.omission {
            None => true,
            Some(Omission::DuplicateEarlier(basis) | Omission::DuplicateLater(basis)) => {
                source_like(current.observation.kind)
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.observation.nonce == basis
                            && other.proposed.omission.is_none()
                            && equivalent(&current.observation, &other.observation)
                            && match saved.omission {
                                Some(Omission::DuplicateEarlier(_)) => {
                                    other.cacheable && other.index < current.index
                                }
                                _ => other.cacheable && other.index > current.index,
                            }
                    })
            }
            Some(Omission::HistoricalSource) => {
                source_like(current.observation.kind)
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.index > current.index
                            && other.observation.kind == ObservationKind::Mutation
                            && affects(&other.observation, &current.observation)
                    })
            }
            Some(Omission::Age) => false, // Only this request's age pass may select it again.
        };
        let notes_valid = saved.notes.iter().all(|note| match note {
            Note::HistoricalCommand => {
                current.observation.kind == ObservationKind::Execution
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.index > current.index
                            && other.observation.kind == ObservationKind::Mutation
                            && affects(&current.observation, &other.observation)
                    })
            }
            Note::UnknownSource => {
                source_like(current.observation.kind)
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.index > current.index
                            && other.observation.kind == ObservationKind::Execution
                            && affects(&other.observation, &current.observation)
                    })
            }
            Note::PriorSourcesSuperseded => slots.iter().any(|other| {
                other.cacheable
                    && other.index < current.index
                    && source_like(other.observation.kind)
                    && affects(&current.observation, &other.observation)
                    && (current.observation.kind == ObservationKind::Mutation
                        || (source_like(current.observation.kind)
                            && ((current.observation.source_sha256.is_some()
                                && other.observation.source_sha256.is_some()
                                && current.observation.source_sha256
                                    != other.observation.source_sha256)
                                || (current.observation.selection_sha256
                                    == other.observation.selection_sha256
                                    && current.observation.result_sha256
                                        != other.observation.result_sha256))))
            }),
            Note::PriorVerificationInvalidated => {
                current.observation.kind == ObservationKind::Mutation
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.index < current.index
                            && other.observation.kind == ObservationKind::Execution
                            && affects(&other.observation, &current.observation)
                    })
            }
            Note::PriorEffectsUnknown => {
                current.observation.kind == ObservationKind::Execution
                    && slots.iter().any(|other| {
                        other.cacheable
                            && other.index < current.index
                            && source_like(other.observation.kind)
                            && affects(&current.observation, &other.observation)
                    })
            }
            Note::IsolatedCommand => {
                current.observation.kind == ObservationKind::Execution
                    && current.observation.command_copy.is_some()
            }
            Note::CopyArtifact => {
                source_like(current.observation.kind) && current.observation.command_copy.is_some()
            }
            Note::SavedUnknown => true,
        });
        if omission_valid && notes_valid {
            slots[i].proposed = saved;
            slots[i].frozen = true;
        }
    }
    let mut later_mutations = Vec::<ObservationV1>::new();
    let mut later_executions = Vec::<ObservationV1>::new();
    let mut latest = HashMap::<(String, String), ObservationV1>::new();
    let mut versions = HashMap::<(String, String), String>::new();
    for i in (0..slots.len()).rev() {
        let observation = slots[i].observation.clone();
        if semantic && !slots[i].frozen && slots[i].cacheable {
            if observation.command_copy.is_some() {
                if observation.kind == ObservationKind::Execution {
                    slots[i].proposed.notes.push(Note::IsolatedCommand);
                } else if source_like(observation.kind) {
                    slots[i].proposed.notes.push(Note::CopyArtifact);
                }
            }
            if observation.kind == ObservationKind::Execution
                && later_mutations
                    .iter()
                    .any(|later| affects(&observation, later))
            {
                slots[i].proposed.notes.push(Note::HistoricalCommand);
            } else if source_like(observation.kind) {
                let historical = later_mutations
                    .iter()
                    .any(|later| affects(later, &observation))
                    || observation.source_sha256.as_ref().is_some_and(|version| {
                        observation.subjects_sha256.iter().any(|subject| {
                            versions
                                .get(&(observation.workspace_sha256.clone(), subject.clone()))
                                .is_some_and(|later| later != version)
                        })
                    });
                let uncertain = later_executions
                    .iter()
                    .any(|later| affects(later, &observation));
                let earlier = slots
                    .iter()
                    .find(|other| {
                        other.index < slots[i].index
                            && other.cacheable
                            && other.frozen
                            && other.proposed.omission.is_none()
                            && equivalent(&observation, &other.observation)
                    })
                    .map(|other| other.observation.nonce);
                let key = (
                    observation.workspace_sha256.clone(),
                    observation.selection_sha256.clone(),
                );
                if historical {
                    slots[i].proposed.omission = Some(Omission::HistoricalSource);
                } else if let Some(basis) = earlier {
                    slots[i].proposed.omission = Some(Omission::DuplicateEarlier(basis));
                } else if !uncertain {
                    if let Some(later) = latest
                        .get(&key)
                        .filter(|later| equivalent(&observation, later))
                    {
                        slots[i].proposed.omission = Some(Omission::DuplicateLater(later.nonce));
                    }
                }
                if uncertain && slots[i].proposed.omission.is_none() {
                    slots[i].proposed.notes.push(Note::UnknownSource);
                }
            }
            let prior_sources = slots.iter().any(|other| {
                other.index < slots[i].index
                    && other.cacheable
                    && source_like(other.observation.kind)
                    && affects(&observation, &other.observation)
            });
            if observation.kind == ObservationKind::Mutation {
                if prior_sources {
                    slots[i].proposed.notes.push(Note::PriorSourcesSuperseded);
                }
                if slots.iter().any(|other| {
                    other.index < slots[i].index
                        && other.cacheable
                        && other.observation.kind == ObservationKind::Execution
                        && affects(&other.observation, &observation)
                }) {
                    slots[i]
                        .proposed
                        .notes
                        .push(Note::PriorVerificationInvalidated);
                }
            } else if observation.kind == ObservationKind::Execution && prior_sources {
                slots[i].proposed.notes.push(Note::PriorEffectsUnknown);
            } else if source_like(observation.kind)
                && slots.iter().any(|other| {
                    other.index < slots[i].index
                        && other.cacheable
                        && affects(&observation, &other.observation)
                        && source_like(other.observation.kind)
                        && ((observation.source_sha256.is_some()
                            && other.observation.source_sha256.is_some()
                            && observation.source_sha256 != other.observation.source_sha256)
                            || (observation.selection_sha256 == other.observation.selection_sha256
                                && observation.result_sha256 != other.observation.result_sha256))
                })
            {
                slots[i].proposed.notes.push(Note::PriorSourcesSuperseded);
            }
        }
        if !slots[i].frozen
            && matches!(
                slots[i].proposed.omission,
                Some(Omission::DuplicateEarlier(_) | Omission::DuplicateLater(_))
            )
        {
            let marker = slots[i].proposed.render(
                &slots[i].original,
                &slots[i].name,
                &slots[i].id,
                basis_call(&slots[i].proposed, &slots),
            );
            let original = slots[i].original.as_str().unwrap_or_default();
            let projected = marker.as_str().unwrap_or_default();
            if projected.len() >= original.len()
                || crate::context::estimate_text_tokens(projected)
                    >= crate::context::estimate_text_tokens(original)
            {
                slots[i].proposed.omission = None;
            }
        }
        if !slots[i].cacheable {
            messages[slots[i].index].content = slots[i].proposed.render(
                &slots[i].original,
                &slots[i].name,
                &slots[i].id,
                basis_call(&slots[i].proposed, &slots),
            );
            continue;
        }
        match observation.kind {
            ObservationKind::Mutation => later_mutations.push(observation.clone()),
            ObservationKind::Execution => later_executions.push(observation.clone()),
            _ => {}
        }
        if source_like(observation.kind) && slots[i].proposed.omission.is_none() {
            latest
                .entry((
                    observation.workspace_sha256.clone(),
                    observation.selection_sha256.clone(),
                ))
                .or_insert_with(|| observation.clone());
        }
        if let Some(version) = &observation.source_sha256 {
            for subject in &observation.subjects_sha256 {
                versions
                    .entry((observation.workspace_sha256.clone(), subject.clone()))
                    .or_insert_with(|| version.clone());
            }
        }
        messages[slots[i].index].content = slots[i].proposed.render(
            &slots[i].original,
            &slots[i].name,
            &slots[i].id,
            basis_call(&slots[i].proposed, &slots),
        );
    }
    let lookup = slots
        .iter()
        .enumerate()
        .map(|(position, slot)| (slot.index, position))
        .collect();
    EvidenceProjection { slots, lookup }
}

#[cfg(test)]
pub fn project(
    messages: &mut [crate::agent::client::ChatMessage],
) -> crate::context_pruning::PruningStats {
    project_deferred(messages, true).finish(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(
        id: &str,
        name: &str,
        arguments: &str,
        output: &str,
        subject: &str,
    ) -> Vec<crate::agent::client::ChatMessage> {
        use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
        use crate::image_content::ModelContent;
        let observation = capture_host(
            name,
            arguments,
            output,
            true,
            "workspace",
            &[subject.into()],
            None,
        );
        vec![
            ChatMessage {
                role: "assistant".into(),
                content: ModelContent::text(""),
                tool_calls: Some(vec![ToolCallMsg {
                    id: id.into(),
                    call_type: "function".into(),
                    function: FunctionCall {
                        name: name.into(),
                        arguments: arguments.into(),
                        arguments_retrieved_spans: vec![],
                    },
                }]),
                tool_call_id: None,
                response_items: vec![],
            },
            ChatMessage {
                role: "tool".into(),
                content: ModelContent::text(output).with_context_observation(observation),
                tool_calls: None,
                tool_call_id: Some(id.into()),
                response_items: vec![],
            },
        ]
    }

    #[test]
    fn hrs317_observation_binds_exact_tool_bytes_and_hides_paths() {
        let observation = capture_host(
            "read_file",
            r#"{"path":"private.rs"}"#,
            "source bytes",
            true,
            "/private/project",
            &["/private/project/private.rs".into()],
            None,
        )
        .unwrap();
        assert!(observation.matches("read_file", r#"{"path":"private.rs"}"#, "source bytes"));
        assert!(!observation.matches("read_file", r#"{"path":"private.rs"}"#, "edited"));
        assert!(observation.live());
        let encoded = serde_json::to_string(&observation).unwrap();
        assert!(!encoded.contains("private.rs"));
        assert!(!encoded.contains("/private/project"));
        let mut forged = observation.clone();
        forged.subjects_sha256.clear();
        assert!(!forged.live());
    }

    #[test]
    fn hrs317_failed_reads_are_not_observations_and_execution_is_unknown() {
        assert!(capture_host("read_file", "{}", "error", false, "w", &[], None).is_none());
        let execution =
            capture_host("run_command", "{}", "timed out", false, "w", &[], None).unwrap();
        assert_eq!(execution.kind, ObservationKind::Execution);
    }

    #[test]
    fn hrs317_duplicate_projection_never_expands_a_short_result() {
        let mut messages = exchange("one", "read_file", "{}", "A", "file");
        messages.extend(exchange("two", "read_file", "{}", "A", "file"));
        let before = serde_json::to_value(&messages).unwrap();
        let stats = project(&mut messages);
        assert_eq!(serde_json::to_value(&messages).unwrap(), before);
        assert_eq!(stats.results, 0);
        assert_eq!(stats.added_tokens, 0);
    }

    fn scoped_exchange(
        id: &str,
        name: &str,
        output: &str,
        root: &str,
    ) -> Vec<crate::agent::client::ChatMessage> {
        let mut messages = exchange(id, name, "{}", output, "file");
        let mut observation = messages[1].content.context_observation().unwrap().clone();
        observation.command_copy = Some(CommandCopyEvidence {
            generation_sha256: digest("generation"),
            root_sha256: digest(root),
            original_subject_sha256: digest("original"),
        });
        register(&observation);
        messages[1].content = messages[1]
            .content
            .clone()
            .with_context_observation(Some(observation));
        messages
    }

    #[test]
    fn hrs317_isolated_command_preserves_original_evidence_and_labels_copy() {
        let mut canonical = exchange("source", "read_file", "{}", "original", "file");
        let mut first = canonical.clone();
        project(&mut first);
        canonical.extend(scoped_exchange(
            "command",
            "run_command",
            "passed",
            "copy-root-a",
        ));
        canonical.extend(scoped_exchange(
            "copy",
            "read_file",
            "copy bytes",
            "copy-root-b",
        ));
        let mut request = canonical.clone();
        project(&mut request);
        assert_eq!(request[1].content.as_str(), Some("original"));
        assert_eq!(
            serde_json::to_value(&request[..first.len()]).unwrap(),
            serde_json::to_value(first).unwrap()
        );
        let command = request[3].content.as_str().unwrap();
        assert!(command.contains("isolated writable copy"));
        assert!(command.contains("Reuse retained original reads unless independently invalidated"));
        assert!(!command.contains("may have changed previously"));
        let copy = request[5].content.as_str().unwrap();
        assert!(copy.contains("command-copy artifact"));
        assert!(copy.contains("Use retained original reads for original claims when still valid"));
        // A command in one admitted root can change another root in that generation.
        assert!(affects(
            canonical[3].content.context_observation().unwrap(),
            canonical[5].content.context_observation().unwrap()
        ));
        // Unknown/unisolated commands retain their broad freshness barrier.
        canonical.extend(exchange(
            "unknown",
            "run_command",
            "{}",
            "unknown effects",
            "",
        ));
        project(&mut canonical);
        assert!(canonical
            .last()
            .unwrap()
            .content
            .as_str()
            .unwrap()
            .contains("freshness is unknown"));
    }

    #[test]
    fn hrs317_saved_copy_scope_cannot_be_revalidated_by_file_digest() {
        let messages = scoped_exchange("copy", "read_file", "copy bytes", "root");
        let observation = messages[1].content.context_observation().unwrap();
        let mut saved: ObservationV1 =
            serde_json::from_str(&serde_json::to_string(observation).unwrap()).unwrap();
        saved.nonce = Uuid::new_v4();
        saved.source_sha256 = Some(digest("copy bytes"));
        assert!(!saved.live());
        assert!(!saved.revalidate_source(&digest("copy bytes")));
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("copy bytes"));
    }

    #[test]
    fn hrs317_restored_metadata_requires_process_provenance() {
        let mut observation =
            capture_host("read_file", "{}", "text", true, "w", &[], None).unwrap();
        observation.nonce = Uuid::new_v4();
        assert!(!observation.live());
        assert!(!observation.revalidate_source(&digest("text")));
    }

    #[test]
    fn hrs317_distinct_evidence_survives_repeated_reads_and_canonical_stays_exact() {
        let mut canonical = Vec::new();
        for (id, path) in [
            ("a1", "A"),
            ("b", "B"),
            ("c", "C"),
            ("a2", "A"),
            ("a3", "A"),
        ] {
            canonical.extend(exchange(
                id,
                "read_file",
                &format!(r#"{{"path":"{path}"}}"#),
                &path.repeat(512),
                path,
            ));
        }
        let original = serde_json::to_vec(&canonical).unwrap();
        let mut request = canonical.clone();
        let stats = project(&mut request);
        assert_eq!(stats.results, 2);
        assert!(request[1].content.is_context_marker());
        assert_eq!(request[3].content.as_str(), Some("B".repeat(512).as_str()));
        assert_eq!(request[5].content.as_str(), Some("C".repeat(512).as_str()));
        assert_eq!(request[9].content.as_str(), Some("A".repeat(512).as_str()));
        assert_eq!(serde_json::to_vec(&canonical).unwrap(), original);
        for (before, after) in canonical.iter().zip(&request) {
            assert_eq!(before.tool_call_id, after.tool_call_id);
            assert_eq!(
                serde_json::to_value(&before.tool_calls).unwrap(),
                serde_json::to_value(&after.tool_calls).unwrap()
            );
        }
    }

    #[test]
    fn hrs317_incremental_evidence_keeps_every_previous_presentation() {
        let mut canonical = Vec::new();
        let mut previous = Vec::new();
        for (id, path) in [
            ("a1", "A"),
            ("b", "B"),
            ("c", "C"),
            ("a2", "A"),
            ("a3", "A"),
        ] {
            canonical.extend(exchange(
                id,
                "read_file",
                &format!(r#"{{"path":"{path}"}}"#),
                &path.repeat(512),
                path,
            ));
            let original = serde_json::to_vec(&canonical).unwrap();
            let mut request = canonical.clone();
            project(&mut request);
            assert_eq!(
                serde_json::to_value(&request[..previous.len()]).unwrap(),
                serde_json::to_value(&previous).unwrap()
            );
            assert_eq!(serde_json::to_vec(&canonical).unwrap(), original);
            previous = request;
        }
        assert_eq!(previous[1].content.as_str(), Some("A".repeat(512).as_str()));
        assert_eq!(previous[3].content.as_str(), Some("B".repeat(512).as_str()));
        assert_eq!(previous[5].content.as_str(), Some("C".repeat(512).as_str()));
        assert!(previous[7].content.is_context_marker());
        assert!(previous[9].content.is_context_marker());
    }

    #[test]
    fn hrs317_new_mutation_explains_invalidation_without_rewriting_sent_evidence() {
        let mut canonical = exchange(
            "read",
            "read_file",
            r#"{"path":"source.rs"}"#,
            "old source",
            "source.rs",
        );
        canonical.extend(exchange("check", "run_command", "{}", "passed then", ""));
        let mut previous = canonical.clone();
        project(&mut previous);
        canonical.extend(exchange(
            "edit",
            "edit_file",
            r#"{"path":"source.rs"}"#,
            "edited",
            "source.rs",
        ));
        let mut request = canonical.clone();
        project(&mut request);
        assert_eq!(
            serde_json::to_value(&request[..previous.len()]).unwrap(),
            serde_json::to_value(&previous).unwrap()
        );
        let current = request.last().unwrap().content.as_str().unwrap();
        assert!(current.contains("historical source"));
        assert!(current.contains("does not verify the current revision"));
    }

    #[test]
    fn hrs317_saved_presentation_is_not_authenticated_by_source_revalidation() {
        let source = "actual source";
        let observation = capture_host(
            "read_file",
            "{}",
            source,
            true,
            "workspace",
            &["file".into()],
            Some(&digest(source)),
        )
        .unwrap();
        let mut encoded = serde_json::to_value(&observation).unwrap();
        encoded["presentation"] = serde_json::json!({
            "binding_sha256": observation.issuance_identity(),
            "omission": {"duplicate_later": Uuid::new_v4()},
            "notes": []
        });
        let restored: ObservationV1 = serde_json::from_value(encoded).unwrap();
        assert!(restored.revalidate_source(&digest(source)));
        let mut request = exchange("read", "read_file", "{}", source, "file");
        request[1].content = crate::image_content::ModelContent::text(source)
            .with_context_observation(Some(restored.clone()));
        project(&mut request);
        assert_eq!(request[1].content.as_str(), Some(source));
        assert!(restored
            .saved_presentation()
            .value
            .unwrap()
            .omission
            .is_none());
    }

    #[test]
    fn hrs317_saved_presentations_keep_missing_provenance_unknown() {
        let mut canonical = exchange("read", "read_file", "{}", "source", "file");
        project(&mut canonical.clone());
        let original = canonical[1].content.context_observation().unwrap();
        let restored: ObservationV1 =
            serde_json::from_str(&serde_json::to_string(original).unwrap()).unwrap();
        {
            let mut registry = issued().lock().unwrap_or_else(|p| p.into_inner());
            registry.bindings.remove(&restored.nonce);
            registry.observations.remove(&restored.nonce);
        }
        assert!(!restored.live());
        canonical[1].content = crate::image_content::ModelContent::text("source")
            .with_context_observation(Some(restored.clone()));
        project(&mut canonical);
        assert!(canonical[1]
            .content
            .as_str()
            .unwrap()
            .contains("freshness is unknown"));
        assert!(restored.saved_presentation().local);
        assert!(!restored.saved_presentation().host);
        assert!(!restored.live());
    }

    #[test]
    fn hrs317_saved_choices_cannot_invent_semantic_changes_or_omit_commands() {
        for (name, note) in [
            ("read_file", true),
            ("run_command", false),
            ("git_status", false),
        ] {
            let output = "same evidence";
            let mut request = exchange("basis", name, "{}", output, "file");
            let mut current = exchange("current", name, "{}", output, "file");
            let observation = current[1].content.context_observation().unwrap();
            let basis = request[1].content.context_observation().unwrap().nonce;
            let mut encoded = serde_json::to_value(observation).unwrap();
            encoded["presentation"] = serde_json::json!({
                "binding_sha256": observation.issuance_identity(),
                "omission": if note { serde_json::Value::Null } else { serde_json::json!({"duplicate_earlier": basis}) },
                "notes": if note { serde_json::json!(["prior_sources_superseded"]) } else { serde_json::json!([]) }
            });
            let forged: ObservationV1 = serde_json::from_value(encoded).unwrap();
            current[1].content = crate::image_content::ModelContent::text(output)
                .with_context_observation(Some(forged.clone()));
            request.extend(current);
            project(&mut request);
            let view = forged.saved_presentation().value.unwrap();
            assert!(!view.notes.contains(&Note::PriorSourcesSuperseded));
            if name != "read_file" {
                assert!(view.omission.is_none());
            }
        }
    }

    #[test]
    fn hrs317_identical_completed_calls_have_separate_presentation_boundaries() {
        capture_host(
            "read_file",
            "{}",
            &"same".repeat(128),
            true,
            "workspace",
            &["file".into()],
            None,
        )
        .unwrap();
        let first = bind_native_result("read_file", "{}", &"same".repeat(128)).unwrap();
        let second = bind_native_result("read_file", "{}", &"same".repeat(128)).unwrap();
        assert_ne!(first.nonce, second.nonce);
        let mut request = exchange("first", "read_file", "{}", &"same".repeat(128), "file");
        request[1].content = crate::image_content::ModelContent::text("same".repeat(128))
            .with_context_observation(Some(first));
        let mut next = exchange("second", "read_file", "{}", &"same".repeat(128), "file");
        next[1].content = crate::image_content::ModelContent::text("same".repeat(128))
            .with_context_observation(Some(second));
        request.extend(next);
        project(&mut request);
        assert!(request[1].content.is_context_marker());
        assert_eq!(
            request[3].content.as_str(),
            Some("same".repeat(128).as_str())
        );
    }

    #[test]
    fn hrs317_failed_read_never_reuses_a_completed_read_receipt() {
        capture_host(
            "read_file",
            "{}",
            "same",
            true,
            "workspace",
            &["file".into()],
            None,
        )
        .unwrap();
        let mut call = crate::app::ToolCall {
            observation: None,
            output_source: None,
            id: "first".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
            arguments_retrieved_spans: vec![],
            status: crate::app::ToolStatus::Error,
            output: Some("same".into()),
            started_at: None,
        };
        call.bind_native_observation();
        assert!(call.observation.is_none());
        call.status = crate::app::ToolStatus::Done;
        call.bind_native_observation();
        let boundary = call.observation.as_ref().unwrap().nonce;
        call.bind_native_observation();
        assert_eq!(call.observation.as_ref().unwrap().nonce, boundary);
        capture_host(
            "run_command",
            "{}",
            "invocation failed",
            false,
            "workspace",
            &[],
            None,
        )
        .unwrap();
        call.name = "run_command".into();
        call.status = crate::app::ToolStatus::Error;
        call.output = Some("invocation failed".into());
        call.bind_native_observation();
        assert_eq!(
            call.observation.as_ref().unwrap().kind,
            ObservationKind::Execution
        );
    }

    #[test]
    fn hrs317_competing_presentations_render_the_committed_winner() {
        let canonical = exchange("first", "read_file", "{}", "same", "file");
        let mut competing = canonical.clone();
        competing.extend(exchange("second", "read_file", "{}", "same", "file"));
        let pending = project_deferred(&mut competing, true);
        let mut first = canonical.clone();
        project(&mut first);
        let stats = pending.finish(&mut competing);
        assert_eq!(
            serde_json::to_value(&competing[..first.len()]).unwrap(),
            serde_json::to_value(&first).unwrap()
        );
        assert_eq!(stats.results, 0);
        assert_eq!(stats.saved_tokens, 0);
        assert_eq!(competing[3].content.as_str(), Some("same"));
    }

    #[test]
    fn hrs317_unknown_presentation_does_not_change_after_host_revalidation() {
        let source = "source";
        let observation = capture_host(
            "read_file",
            "{}",
            source,
            true,
            "workspace",
            &["file".into()],
            Some(&digest(source)),
        )
        .unwrap();
        let restored: ObservationV1 =
            serde_json::from_str(&serde_json::to_string(&observation).unwrap()).unwrap();
        {
            let mut registry = issued().lock().unwrap_or_else(|p| p.into_inner());
            registry.bindings.remove(&restored.nonce);
            registry.observations.remove(&restored.nonce);
        }
        let mut canonical = exchange("read", "read_file", "{}", source, "file");
        canonical[1].content = crate::image_content::ModelContent::text(source)
            .with_context_observation(Some(restored.clone()));
        let mut before = canonical.clone();
        project(&mut before);
        assert!(before[1]
            .content
            .as_str()
            .unwrap()
            .contains("freshness is unknown"));
        assert!(restored.revalidate_source(&digest(source)));
        let mut after = canonical.clone();
        project(&mut after);
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }

    #[test]
    fn hrs317_age_pruning_freezes_at_the_same_presentation_boundary() {
        use crate::agent::client::ChatMessage;
        use crate::image_content::ModelContent;
        let mut canonical = Vec::new();
        let user = || ChatMessage {
            role: "user".into(),
            content: ModelContent::text("inspect"),
            tool_calls: None,
            tool_call_id: None,
            response_items: vec![],
        };
        for i in 0..5 {
            canonical.push(user());
            canonical.extend(exchange(
                &format!("read-{i}"),
                "read_file",
                &format!(r#"{{"path":"{i}"}}"#),
                &"evidence ".repeat(8_000),
                &i.to_string(),
            ));
            let result = canonical.last_mut().unwrap();
            result.content = result.content.clone().with_prunable_tool_result(true);
        }
        let mut first = canonical.clone();
        assert!(crate::context_pruning::project(&mut first, true, Some(8_000)).results > 0);
        canonical.push(user());
        canonical.extend(exchange(
            "new",
            "read_file",
            r#"{"path":"new"}"#,
            "new evidence",
            "new",
        ));
        let mut next = canonical.clone();
        crate::context_pruning::project(&mut next, true, Some(8_000));
        assert_eq!(
            serde_json::to_value(&next[..first.len()]).unwrap(),
            serde_json::to_value(&first).unwrap()
        );
    }

    #[test]
    fn hrs317_edits_supersede_source_and_make_previous_commands_historical() {
        let mut request = exchange(
            "r",
            "read_file",
            r#"{"path":"source.rs"}"#,
            "old source",
            "source.rs",
        );
        request.extend(exchange(
            "v",
            "run_command",
            r#"{"command":"check"}"#,
            "passed then",
            "",
        ));
        request.extend(exchange(
            "unrelated",
            "read_file",
            "{}",
            "still valid",
            "other.rs",
        ));
        request.extend(exchange(
            "e",
            "edit_file",
            r#"{"path":"source.rs"}"#,
            "edited",
            "source.rs",
        ));
        project(&mut request);
        assert!(request[1].content.is_context_marker());
        assert!(request[1].content.as_str().unwrap().contains("historical"));
        assert!(request[3]
            .content
            .as_str()
            .unwrap()
            .contains("does not verify the current revision"));
        let edit = request[7].content.as_str().unwrap();
        assert!(edit.contains("Read the changed source for its current revision"));
        assert!(edit.contains("Reuse unaffected evidence when still valid"));
        assert_eq!(request[5].content.as_str(), Some("still valid"));
    }

    #[test]
    fn hrs317_unknown_execution_preserves_source_instead_of_claiming_drift() {
        let mut request = exchange("r", "read_file", "{}", "source evidence", "source.rs");
        request.extend(exchange("x", "run_command", "{}", "timeout", ""));
        project(&mut request);
        assert!(!request[1].content.is_context_marker());
        assert!(request[1]
            .content
            .as_str()
            .unwrap()
            .starts_with("source evidence"));
        assert!(request[1]
            .content
            .as_str()
            .unwrap()
            .contains("freshness is unknown"));
    }

    #[test]
    fn hrs317_partial_batch_is_protected_from_semantic_pruning() {
        let mut request = exchange("a", "list_dir", "{}", "entries", "root");
        let mut missing = request[0].tool_calls.as_ref().unwrap()[0].clone();
        missing.id = "missing".into();
        request[0].tool_calls.as_mut().unwrap().push(missing);
        request.extend(exchange("b", "list_dir", "{}", "entries", "root"));
        assert_eq!(project(&mut request).results, 0);
        assert_eq!(request[1].content.as_str(), Some("entries"));
    }

    #[test]
    fn hrs317_invalid_metadata_does_not_break_saved_calls_or_change_requests() {
        #[derive(Deserialize)]
        struct Saved {
            #[serde(default, deserialize_with = "deserialize_optional")]
            observation: Option<ObservationV1>,
        }
        let saved: Saved = serde_json::from_str(r#"{"observation":{"version":999}}"#).unwrap();
        assert!(saved.observation.is_none());
        let mut request = exchange("r", "read_file", "{}", "text", "file");
        request[1].content = crate::image_content::ModelContent::text("different bytes")
            .with_context_observation(request[1].content.context_observation().cloned());
        assert_eq!(project(&mut request).results, 0);
        assert_eq!(request[1].content.as_str(), Some("different bytes"));
    }

    #[tokio::test]
    async fn hrs317_resume_revalidates_exact_source_and_rejects_changed_disk() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let path = root.join("source.rs");
        std::fs::write(&path, "original").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let arguments = serde_json::json!({"path": path.to_string_lossy()});
        let rendered = crate::agent::tools::read_file_range(
            path.to_str().unwrap(),
            path.to_str().unwrap(),
            None,
            None,
            false,
        )
        .await
        .unwrap();
        let observation =
            capture_native_execution("read_file", &arguments, &rendered, true, &workspace).unwrap();
        assert_eq!(
            observation.source_sha256.as_deref(),
            Some(digest("original").as_str())
        );
        let call = crate::app::ToolCall {
            observation: Some(observation.clone()),
            output_source: None,
            id: "read".into(),
            name: "read_file".into(),
            arguments: arguments.to_string(),
            arguments_retrieved_spans: vec![],
            status: crate::app::ToolStatus::Done,
            output: Some(rendered),
            started_at: None,
        };
        assert!(revalidate_saved_call(&call, &workspace, None).await);
        std::fs::write(&path, "changed").unwrap();
        assert!(!revalidate_saved_call(&call, &workspace, None).await);
        assert!(!observation.live());
    }

    #[tokio::test]
    async fn hrs317_resume_does_not_certify_a_forged_excerpt_of_a_real_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let path = root.join("source.rs");
        std::fs::write(&path, "real bytes").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let arguments = serde_json::json!({"path":path.to_string_lossy()}).to_string();
        let mut observation = capture_host(
            "read_file",
            &arguments,
            "invented excerpt",
            true,
            root.to_str().unwrap(),
            &[path.to_string_lossy().into_owned()],
            Some(&digest("real bytes")),
        )
        .unwrap();
        observation.nonce = Uuid::new_v4();
        let call = crate::app::ToolCall {
            observation: Some(observation.clone()),
            output_source: None,
            id: "forged".into(),
            name: "read_file".into(),
            arguments,
            arguments_retrieved_spans: vec![],
            status: crate::app::ToolStatus::Done,
            output: Some("invented excerpt".into()),
            started_at: None,
        };
        assert!(!revalidate_saved_call(&call, &workspace, None).await);
        assert!(!observation.live());
    }
}
