use nucleo::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const MAX_DISCOVERED_PATHS: usize = 50_000;
pub const MAX_PICKER_MATCHES: usize = 250;
pub const MAX_FRECENCY_RECORDS: usize = 1_000;
pub const MAX_COMPOSER_ATTACHMENTS: usize = 32;
pub const MAX_PICKER_QUERY_BYTES: usize = 512;
const MAX_MENTIONS: usize = 10;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_ATTACHED_CHARS: usize = 10_000;
const MAX_RESOURCE_CONTEXT_BYTES: usize = 64 * 1024;
const MAX_FRECENCY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerPhase {
    #[default]
    Idle,
    Indexing,
    Matching,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSpec {
    pub id: String,
    pub path: PathBuf,
    pub label: String,
    pub read_only: bool,
    pub primary: bool,
}

impl RootSpec {
    pub fn from_workspace(workspace: &crate::workspace::WorkspaceContext) -> Vec<Self> {
        let mut labels = HashSet::new();
        workspace
            .roots()
            .iter()
            .map(|root| {
                let base = if root.primary {
                    "workspace".to_string()
                } else {
                    root.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .unwrap_or("root")
                        .to_string()
                };
                let safe = bounded_label(&base, 48);
                let label = if labels.insert(safe.clone()) {
                    safe
                } else {
                    format!("{}-{}", safe, &root.id.to_string()[..8])
                };
                Self {
                    id: root.id.to_string(),
                    path: root.path.clone(),
                    label,
                    read_only: root.read_only,
                    primary: root.primary,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub root_id: String,
    pub root_label: String,
    pub relative: String,
    pub reference: String,
    pub read_only: bool,
}

impl IndexedFile {
    pub fn frecency_key(&self) -> FrecencyKey {
        FrecencyKey {
            root_id: self.root_id.clone(),
            relative: self.relative.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileIndexSnapshot {
    pub generation: u64,
    pub files: Arc<Vec<IndexedFile>>,
    pub discovered: usize,
    pub truncated: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEntry {
    File(IndexedFile),
    Mcp(crate::mcp::McpResourceInfo),
}

impl PickerEntry {
    pub fn display(&self) -> String {
        bounded_label(
            &match self {
                Self::File(file) => format!(
                    "{}{}  {}",
                    if file.read_only { "[read-only] " } else { "" },
                    file.root_label,
                    file.relative
                ),
                Self::Mcp(resource) => format!("{}  {}", resource.server, resource.name),
            },
            512,
        )
    }
}

#[derive(Debug, Clone)]
pub struct MatchSnapshot {
    pub generation: u64,
    pub entries: Vec<PickerEntry>,
    pub total_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrecencyKey {
    pub root_id: String,
    pub relative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrecencyRecord {
    pub uses: u32,
    pub last_used_unix: i64,
}

pub type FrecencyMap = BTreeMap<String, FrecencyRecord>;

fn frecency_identity(key: &FrecencyKey) -> String {
    format!("{}\0{}", key.root_id, key.relative)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrecencyFile {
    version: u8,
    records: BTreeMap<String, FrecencyRecord>,
}

#[derive(Debug, Clone)]
pub struct FrecencyStore {
    path: PathBuf,
    writable: bool,
}

impl FrecencyStore {
    pub fn new(profile_data: &Path, writable: bool) -> Self {
        Self {
            path: profile_data.join("attachments/frecency.json"),
            writable,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            path: PathBuf::new(),
            writable: false,
        }
    }

    pub fn load(&self) -> Result<FrecencyMap, String> {
        if self.path.as_os_str().is_empty() || !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|error| format!("could not inspect attachment ranking data: {error}"))?;
        if !metadata.file_type().is_file() || metadata.len() as usize > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data is not a bounded regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.mode() & 0o077 != 0 {
                return Err("attachment ranking data is not owner-only".into());
            }
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .map_err(|error| error.to_string())?
            .take((MAX_FRECENCY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data exceeds 256 KiB".into());
        }
        let file: FrecencyFile = serde_json::from_slice(&bytes)
            .map_err(|_| "attachment ranking data is invalid".to_string())?;
        if file.version != 1 || file.records.len() > MAX_FRECENCY_RECORDS {
            return Err("attachment ranking data has an unsupported shape".into());
        }
        for identity in file.records.keys() {
            validate_frecency_identity(identity)?;
        }
        Ok(file.records)
    }

    pub fn record(&self, key: &FrecencyKey, now_unix: i64) -> Result<FrecencyMap, String> {
        let identity = frecency_identity(key);
        validate_frecency_identity(&identity)?;
        let mut records = self.load().unwrap_or_default();
        let record = records.entry(identity).or_insert(FrecencyRecord {
            uses: 0,
            last_used_unix: now_unix,
        });
        record.uses = record.uses.saturating_add(1);
        record.last_used_unix = now_unix;
        while records.len() > MAX_FRECENCY_RECORDS {
            let Some(oldest) = records
                .iter()
                .min_by_key(|(identity, record)| {
                    (record.last_used_unix, record.uses, identity.as_str())
                })
                .map(|(identity, _)| identity.clone())
            else {
                break;
            };
            records.remove(&oldest);
        }
        if self.writable {
            self.save(&records)?;
        }
        Ok(records)
    }

    fn save(&self, records: &FrecencyMap) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&FrecencyFile {
            version: 1,
            records: records.clone(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > MAX_FRECENCY_BYTES {
            return Err("attachment ranking data exceeds 256 KiB".into());
        }
        let parent = self
            .path
            .parent()
            .ok_or("attachment ranking path has no parent")?;
        if parent.exists()
            && fs::symlink_metadata(parent)
                .map_err(|error| error.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("attachment ranking directory cannot be a symlink".into());
        }
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| error.to_string())?;
        }
        if self.path.exists()
            && fs::symlink_metadata(&self.path)
                .map_err(|error| error.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("attachment ranking data cannot be a symlink".into());
        }
        let temp = parent.join(format!(".frecency-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        let result = (|| {
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            fs::rename(&temp, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn validate_frecency_identity(identity: &str) -> Result<(), String> {
    if identity.is_empty()
        || identity.len() > 4_096
        || identity
            .chars()
            .any(|character| character.is_control() && character != '\0')
        || !identity.contains('\0')
        || identity.split_once('\0').is_none_or(|(root, relative)| {
            root.is_empty()
                || root.len() > 64
                || relative.is_empty()
                || relative.starts_with('/')
                || Path::new(relative)
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
        })
    {
        Err("attachment ranking identity is invalid".into())
    } else {
        Ok(())
    }
}

pub fn discover_files(
    roots: &[RootSpec],
    cwd: &Path,
    generation: u64,
    cancelled: &AtomicBool,
) -> FileIndexSnapshot {
    let mut files = Vec::new();
    let mut discovered = 0usize;
    let mut truncated = false;
    let mut error = None;
    'roots: for root in roots {
        let mut builder = ignore::WalkBuilder::new(&root.path);
        builder
            .standard_filters(true)
            .require_git(false)
            .follow_links(false);
        for item in builder.build() {
            if cancelled.load(Ordering::Relaxed) {
                break 'roots;
            }
            let entry = match item {
                Ok(entry) => entry,
                Err(_) => {
                    error.get_or_insert_with(|| {
                        "Some workspace entries could not be inspected".to_string()
                    });
                    continue;
                }
            };
            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() || file_type.is_symlink() {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(&root.path) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            if discovered == MAX_DISCOVERED_PATHS {
                truncated = true;
                break 'roots;
            }
            let relative = relative.to_string_lossy().replace('\\', "/");
            let reference = entry
                .path()
                .strip_prefix(cwd)
                .ok()
                .filter(|path| !path.as_os_str().is_empty())
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| entry.path().to_string_lossy().into_owned());
            if !safe_picker_path(&relative) || !safe_picker_path(&reference) {
                continue;
            }
            files.push(IndexedFile {
                root_id: root.id.clone(),
                root_label: root.label.clone(),
                relative,
                reference,
                read_only: root.read_only,
            });
            discovered += 1;
        }
    }
    files.sort_by(|left, right| {
        left.root_label
            .cmp(&right.root_label)
            .then_with(|| left.relative.cmp(&right.relative))
    });
    FileIndexSnapshot {
        generation,
        files: Arc::new(files),
        discovered,
        truncated,
        error,
    }
}

fn safe_picker_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

pub fn rank_matches(
    files: &Arc<Vec<IndexedFile>>,
    resources: &[crate::mcp::McpResourceInfo],
    query: &str,
    frecency: &FrecencyMap,
    generation: u64,
    cancelled: &AtomicBool,
) -> MatchSnapshot {
    let (query, _) = split_picker_range(query);
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut scored = Vec::new();
    for file in files.iter() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let mut utf32 = Vec::new();
        let Some(score) = pattern.score(
            nucleo::Utf32Str::new(&file.relative, &mut utf32),
            &mut matcher,
        ) else {
            continue;
        };
        let record = frecency
            .get(&frecency_identity(&file.frecency_key()))
            .copied()
            .unwrap_or(FrecencyRecord {
                uses: 0,
                last_used_unix: 0,
            });
        scored.push((score, record, file));
    }
    scored.sort_by(
        |(left_score, left_record, left), (right_score, right_record, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| right_record.uses.cmp(&left_record.uses))
                .then_with(|| right_record.last_used_unix.cmp(&left_record.last_used_unix))
                .then_with(|| left.root_label.cmp(&right.root_label))
                .then_with(|| left.relative.cmp(&right.relative))
        },
    );
    let mut resource_matches = resources
        .iter()
        .filter_map(|resource| {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            let searchable = format!("{} {} {}", resource.server, resource.name, resource.uri);
            let mut utf32 = Vec::new();
            pattern
                .score(nucleo::Utf32Str::new(&searchable, &mut utf32), &mut matcher)
                .map(|score| (score, resource))
        })
        .collect::<Vec<_>>();
    resource_matches.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.server.cmp(&right.server))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.uri.cmp(&right.uri))
    });
    let total_matches = scored.len().saturating_add(resource_matches.len());
    // Keep a small bounded resource section visible even when a large workspace
    // supplies enough file matches to fill the entire snapshot.
    let resource_limit = resource_matches.len().min(25);
    let file_limit = MAX_PICKER_MATCHES.saturating_sub(resource_limit);
    let mut entries = scored
        .iter()
        .take(file_limit)
        .map(|(_, _, file)| PickerEntry::File((*file).clone()))
        .collect::<Vec<_>>();
    entries.extend(
        resource_matches
            .into_iter()
            .take(resource_limit)
            .map(|(_, resource)| PickerEntry::Mcp(resource.clone())),
    );
    MatchSnapshot {
        generation,
        entries,
        total_matches,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAttachmentKind {
    File {
        root_id: String,
        relative: String,
    },
    Mcp {
        server: String,
        uri: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerAttachment {
    pub reference: String,
    pub kind: ComposerAttachmentKind,
}

impl ComposerAttachment {
    pub fn from_picker(entry: &PickerEntry) -> Self {
        match entry {
            PickerEntry::File(file) => Self {
                reference: file.reference.clone(),
                kind: ComposerAttachmentKind::File {
                    root_id: file.root_id.clone(),
                    relative: file.relative.clone(),
                },
            },
            PickerEntry::Mcp(resource) => {
                let digest =
                    Sha256::digest(format!("{}\0{}", resource.server, resource.uri).as_bytes());
                let short = digest[..8]
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                Self {
                    reference: format!("mcp:{}:{short}", resource.server),
                    kind: ComposerAttachmentKind::Mcp {
                        server: resource.server.clone(),
                        uri: resource.uri.clone(),
                        name: resource.name.clone(),
                    },
                }
            }
        }
    }

    pub fn token(&self) -> String {
        format!("@{}", self.reference)
    }

    pub fn is_referenced_by(&self, input: &str) -> bool {
        input.split_whitespace().any(|token| {
            token == self.token()
                || matches!(&self.kind, ComposerAttachmentKind::File { .. })
                    && token
                        .strip_prefix(&self.token())
                        .is_some_and(|suffix| suffix.starts_with('#'))
        })
    }
}

pub fn has_selected_mcp_attachments(input: &str, selected: &[ComposerAttachment]) -> bool {
    selected.iter().any(|attachment| {
        matches!(&attachment.kind, ComposerAttachmentKind::Mcp { .. })
            && attachment.is_referenced_by(input)
    })
}

pub fn retain_unreferenced(input: &str, selected: &mut Vec<ComposerAttachment>) {
    selected.retain(|attachment| !attachment.is_referenced_by(input));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

pub fn split_picker_range(filter: &str) -> (&str, Option<&str>) {
    let Some((query, range)) = filter.rsplit_once('#') else {
        return (filter, None);
    };
    if range.is_empty()
        || range
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        (query, Some(range))
    } else {
        (filter, None)
    }
}

pub fn parse_line_range(value: &str) -> Result<LineRange, String> {
    let (start, end) = value
        .split_once('-')
        .map_or((value, value), |(start, end)| (start, end));
    if start.is_empty()
        || end.is_empty()
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || !end.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("line range must be #N or #N-M".into());
    }
    let start = start
        .parse::<usize>()
        .map_err(|_| "line range is too large".to_string())?;
    let end = end
        .parse::<usize>()
        .map_err(|_| "line range is too large".to_string())?;
    if start == 0 || end == 0 || start > end {
        return Err("line range must be positive and ordered".into());
    }
    Ok(LineRange { start, end })
}

pub fn range_suffix(filter: &str) -> Result<String, String> {
    let (_, range) = split_picker_range(filter);
    match range {
        None => Ok(String::new()),
        Some("") => Err("finish the line range before selecting a file".into()),
        Some(range) => parse_line_range(range).map(|_| format!("#{range}")),
    }
}

pub fn expand_file_mentions(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<String, String> {
    let mentions = mention_tokens(input)?;
    if mentions.is_empty() {
        return Ok(input.to_string());
    }
    let mut result = input.to_string();
    for mention in mentions.into_iter().take(MAX_MENTIONS) {
        if mention.reference.starts_with("mcp:") {
            continue;
        }
        let selected_file = selected.iter().find_map(|attachment| {
            let ComposerAttachmentKind::File { root_id, relative } = &attachment.kind else {
                return None;
            };
            (attachment.reference == mention.reference)
                .then_some((root_id.as_str(), relative.as_str()))
        });
        let resolved = if let Some((root_id, relative)) = selected_file {
            let root = workspace
                .roots()
                .iter()
                .find(|root| root.id.to_string() == root_id)
                .ok_or_else(|| {
                    format!("attachment root is no longer available: {}", mention.raw)
                })?;
            root.path.join(relative)
        } else {
            let raw_path = Path::new(&mention.reference);
            let lexical = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                workspace.cwd().join(raw_path)
            };
            if fs::symlink_metadata(&lexical).is_ok() {
                let lexical_root = workspace
                    .roots()
                    .iter()
                    .find(|root| lexical.starts_with(&root.path))
                    .ok_or_else(|| {
                        format!("attachment is outside workspace roots: {}", mention.raw)
                    })?;
                reject_symlink_components(&lexical_root.path, &lexical)?;
            }
            match workspace.resolve(&mention.reference, crate::workspace::PathOperation::Read) {
                Ok(path) => path,
                Err(crate::workspace::WorkspaceError::Missing(_)) => {
                    result.push_str(&format!("\n\n(file not found: {})", mention.raw));
                    continue;
                }
                Err(error) => {
                    return Err(format!("attachment rejected: {error}"));
                }
            }
        };
        append_path(&mut result, workspace, &resolved, &mention)?;
    }
    Ok(result)
}

/// Return only host-validated explicit file references from the composer.
/// Ordinary prose is deliberately ignored, even when it contains a path-like
/// substring without an `@` token.
pub fn explicit_file_paths(
    input: &str,
    workspace: &crate::workspace::WorkspaceContext,
    selected: &[ComposerAttachment],
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for mention in mention_tokens(input)?.into_iter().take(MAX_MENTIONS) {
        if mention.reference.starts_with("mcp:") {
            continue;
        }
        let selected_file = selected.iter().find_map(|attachment| {
            let ComposerAttachmentKind::File { root_id, relative } = &attachment.kind else {
                return None;
            };
            (attachment.reference == mention.reference)
                .then_some((root_id.as_str(), relative.as_str()))
        });
        let resolved = if let Some((root_id, relative)) = selected_file {
            let root = workspace
                .roots()
                .iter()
                .find(|root| root.id.to_string() == root_id)
                .ok_or_else(|| {
                    format!("attachment root is no longer available: {}", mention.raw)
                })?;
            let lexical = root.path.join(relative);
            reject_symlink_components(&root.path, &lexical)?;
            fs::canonicalize(&lexical)
                .map_err(|_| format!("attachment is unavailable: {}", mention.raw))?
        } else {
            let raw_path = Path::new(&mention.reference);
            let lexical = if raw_path.is_absolute() {
                raw_path.to_path_buf()
            } else {
                workspace.cwd().join(raw_path)
            };
            if fs::symlink_metadata(&lexical).is_ok() {
                let lexical_root = workspace
                    .roots()
                    .iter()
                    .find(|root| lexical.starts_with(&root.path))
                    .ok_or_else(|| {
                        format!("attachment is outside workspace roots: {}", mention.raw)
                    })?;
                reject_symlink_components(&lexical_root.path, &lexical)?;
            }
            match workspace.resolve(&mention.reference, crate::workspace::PathOperation::Read) {
                Ok(path) => path,
                Err(crate::workspace::WorkspaceError::Missing(_)) => continue,
                Err(error) => return Err(format!("attachment rejected: {error}")),
            }
        };
        if !paths.contains(&resolved) {
            paths.push(resolved);
        }
    }
    Ok(paths)
}

#[derive(Debug)]
struct Mention {
    raw: String,
    reference: String,
    range: Option<LineRange>,
}

fn mention_tokens(input: &str) -> Result<Vec<Mention>, String> {
    let mut mentions = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'@' {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end == index + 1 {
            index = end;
            continue;
        }
        let raw = &input[index + 1..end];
        let (reference, range) = match split_picker_range(raw) {
            (path, Some(suffix)) => (path, Some(parse_line_range(suffix)?)),
            _ => (raw, None),
        };
        mentions.push(Mention {
            raw: raw.to_string(),
            reference: reference.to_string(),
            range,
        });
        index = end;
    }
    Ok(mentions)
}

fn append_path(
    result: &mut String,
    workspace: &crate::workspace::WorkspaceContext,
    resolved: &Path,
    mention: &Mention,
) -> Result<(), String> {
    let root = workspace
        .roots()
        .iter()
        .find(|root| resolved.starts_with(&root.path))
        .ok_or_else(|| format!("attachment is outside workspace roots: {}", mention.raw))?;
    reject_symlink_components(&root.path, resolved)?;
    let metadata = fs::symlink_metadata(resolved)
        .map_err(|_| format!("attachment is unavailable: {}", mention.raw))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("attachment cannot be a symlink: {}", mention.raw));
    }
    if metadata.is_dir() {
        return Err(format!("attachment is not a regular file: {}", mention.raw));
    }
    if !metadata.is_file() {
        return Err(format!("attachment is not a regular file: {}", mention.raw));
    }
    if metadata.len() as usize > MAX_FILE_BYTES {
        return Err(format!("attachment exceeds 1 MiB: {}", mention.raw));
    }
    let bytes =
        fs::read(resolved).map_err(|_| format!("attachment could not be read: {}", mention.raw))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("attachment changed while reading: {}", mention.raw));
    }
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| format!("attachment is not UTF-8 text: {}", mention.raw))?;
    let (body, range_label) = if let Some(range) = mention.range {
        let lines = content
            .split_terminator('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect::<Vec<_>>();
        if range.end > lines.len() {
            return Err(format!(
                "attachment range #{}-{} exceeds {} lines",
                range.start,
                range.end,
                lines.len()
            ));
        }
        (
            lines[range.start - 1..range.end].join("\n"),
            format!(" lines {}-{}", range.start, range.end),
        )
    } else {
        (content.to_string(), String::new())
    };
    let body = body.chars().take(MAX_ATTACHED_CHARS).collect::<String>();
    result.push_str(&format!(
        "\n\n--- Attached file: {}{} ---\n{}\n--- End file ---",
        mention.reference, range_label, body
    ));
    Ok(())
}

fn reject_symlink_components(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "attachment is outside workspace roots".to_string())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("attachment path is invalid".into());
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| "attachment path changed during validation".to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("attachment path cannot contain symlinks".into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceResolution {
    pub process_context: Option<String>,
    pub unavailable_markers: Vec<String>,
}

pub async fn resolve_mcp_attachments(
    input: &str,
    selected: &[ComposerAttachment],
    manager: Option<&Arc<crate::mcp::McpManager>>,
) -> ResourceResolution {
    let mut resolution = ResourceResolution::default();
    let mut context = String::new();
    for attachment in selected.iter().take(MAX_COMPOSER_ATTACHMENTS) {
        if !attachment.is_referenced_by(input) {
            continue;
        }
        let ComposerAttachmentKind::Mcp { server, uri, name } = &attachment.kind else {
            continue;
        };
        let Some(manager) = manager else {
            resolution.unavailable_markers.push(format!(
                "(MCP resource unavailable: {})",
                attachment.reference
            ));
            continue;
        };
        match manager.read_resource(server, uri).await {
            Ok(body) => {
                let section = format!(
                    "\n\n--- MCP resource: {} ({}) ---\n{}\n--- End MCP resource ---",
                    attachment.reference,
                    bounded_label(name, 128),
                    body
                );
                append_utf8_bounded(&mut context, &section, MAX_RESOURCE_CONTEXT_BYTES);
            }
            Err(_) => resolution.unavailable_markers.push(format!(
                "(MCP resource unavailable: {})",
                attachment.reference
            )),
        }
    }
    if !context.is_empty() {
        resolution.process_context = Some(context);
    }
    resolution
}

fn append_utf8_bounded(target: &mut String, value: &str, maximum: usize) {
    if target.len() >= maximum {
        return;
    }
    let mut end = value.len().min(maximum - target.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&value[..end]);
}

fn bounded_label(value: &str, maximum: usize) -> String {
    let safe = crate::terminal_text::sanitize_terminal_text(value);
    if safe.len() <= maximum {
        return safe;
    }
    let mut end = maximum;
    while !safe.is_char_boundary(end) {
        end -= 1;
    }
    safe[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ranges_are_strict_and_picker_suffix_is_separate() {
        assert_eq!(
            parse_line_range("12").unwrap(),
            LineRange { start: 12, end: 12 }
        );
        assert_eq!(
            parse_line_range("12-40").unwrap(),
            LineRange { start: 12, end: 40 }
        );
        for invalid in ["", "0", "4-3", "1-", "-2", "one", "1-2-3"] {
            assert!(parse_line_range(invalid).is_err(), "accepted {invalid:?}");
        }
        assert_eq!(
            split_picker_range("src/main#12-40"),
            ("src/main", Some("12-40"))
        );
        assert_eq!(split_picker_range("src/#name"), ("src/#name", None));
    }

    #[test]
    fn expansion_revalidates_range_regular_file_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("sample.txt"), "one\ntwo\nthree\n").unwrap();
        let workspace =
            crate::workspace::WorkspaceContext::from_primary(root.clone(), root.clone());
        let expanded = expand_file_mentions("check @sample.txt#2-3", &workspace, &[]).unwrap();
        assert!(expanded.contains("lines 2-3"));
        assert!(expanded.contains("two\nthree"));
        assert!(expand_file_mentions("@sample.txt#3-4", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@sample.txt#1-", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@../sample.txt", &workspace, &[]).is_err());
        assert!(expand_file_mentions("@.", &workspace, &[]).is_err());
        fs::write(root.join("large.txt"), vec![b'x'; MAX_FILE_BYTES + 1]).unwrap();
        assert!(expand_file_mentions("@large.txt", &workspace, &[]).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("sample.txt"), root.join("linked.txt")).unwrap();
            assert!(expand_file_mentions("@linked.txt", &workspace, &[]).is_err());
        }
    }

    #[test]
    fn discovery_respects_ignore_multiple_roots_and_cap_cancellation() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::write(first.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(first.path().join("kept.txt"), "ok").unwrap();
        fs::write(first.path().join("ignored.txt"), "no").unwrap();
        fs::write(second.path().join("second.txt"), "ok").unwrap();
        let roots = vec![
            RootSpec {
                id: "one".into(),
                path: first.path().canonicalize().unwrap(),
                label: "workspace".into(),
                read_only: false,
                primary: true,
            },
            RootSpec {
                id: "two".into(),
                path: second.path().canonicalize().unwrap(),
                label: "second".into(),
                read_only: true,
                primary: false,
            },
        ];
        let snapshot = discover_files(&roots, first.path(), 7, &AtomicBool::new(false));
        assert_eq!(snapshot.generation, 7);
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.relative == "kept.txt"));
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.relative == "second.txt" && file.read_only));
        assert!(!snapshot
            .files
            .iter()
            .any(|file| file.relative == "ignored.txt"));

        let cancelled = AtomicBool::new(true);
        assert!(discover_files(&roots, first.path(), 8, &cancelled)
            .files
            .is_empty());
    }

    #[test]
    fn ranking_prefers_fuzzy_score_then_frecency_and_is_bounded() {
        let files = Arc::new(vec![
            IndexedFile {
                root_id: "root".into(),
                root_label: "workspace".into(),
                relative: "src/main.rs".into(),
                reference: "src/main.rs".into(),
                read_only: false,
            },
            IndexedFile {
                root_id: "root".into(),
                root_label: "workspace".into(),
                relative: "misc/main-result-snapshot.txt".into(),
                reference: "misc/main-result-snapshot.txt".into(),
                read_only: false,
            },
        ]);
        let mut frecency = BTreeMap::new();
        frecency.insert(
            frecency_identity(&files[1].frecency_key()),
            FrecencyRecord {
                uses: 9,
                last_used_unix: 99,
            },
        );
        let snapshot = rank_matches(
            &files,
            &[],
            "main.rs",
            &frecency,
            3,
            &AtomicBool::new(false),
        );
        assert_eq!(snapshot.generation, 3);
        assert!(snapshot.entries.len() <= MAX_PICKER_MATCHES);
        assert!(
            matches!(&snapshot.entries[0], PickerEntry::File(file) if file.relative == "src/main.rs")
        );
    }

    #[test]
    fn frecency_is_relative_bounded_and_bare_is_process_only() {
        let temp = tempfile::tempdir().unwrap();
        let store = FrecencyStore::new(temp.path(), true);
        let key = FrecencyKey {
            root_id: "root".into(),
            relative: "src/main.rs".into(),
        };
        let records = store.record(&key, 10).unwrap();
        assert_eq!(records[&frecency_identity(&key)].uses, 1);
        assert!(
            !fs::read_to_string(temp.path().join("attachments/frecency.json"))
                .unwrap()
                .contains(temp.path().to_string_lossy().as_ref())
        );

        let bare = FrecencyStore::unavailable();
        let records = bare.record(&key, 11).unwrap();
        assert_eq!(records[&frecency_identity(&key)].uses, 1);

        fs::write(
            temp.path().join("attachments/frecency.json"),
            b"not valid json",
        )
        .unwrap();
        let recovered = store.record(&key, 12).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(store.load().unwrap(), recovered);

        let mut full = BTreeMap::new();
        for index in 0..MAX_FRECENCY_RECORDS {
            full.insert(
                frecency_identity(&FrecencyKey {
                    root_id: "root".into(),
                    relative: format!("src/{index}.rs"),
                }),
                FrecencyRecord {
                    uses: 1,
                    last_used_unix: index as i64,
                },
            );
        }
        store.save(&full).unwrap();
        let new_key = FrecencyKey {
            root_id: "root".into(),
            relative: "new.rs".into(),
        };
        let bounded = store
            .record(&new_key, MAX_FRECENCY_RECORDS as i64 + 1)
            .unwrap();
        assert_eq!(bounded.len(), MAX_FRECENCY_RECORDS);
        assert!(bounded.contains_key(&frecency_identity(&new_key)));
    }
}
