//! Export local sessions and recovery journals into titled review packs.
//!
//! Titling reads only session metadata (and recovery directory names). Turn
//! transcripts and recovery blobs are copied as opaque bytes and never parsed.

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const REVIEW_PACK_SCHEMA: &str = "ca.session-review-pack.v1";
/// Upper bound for the metadata probe used solely for titling.
const MAX_TITLE_PROBE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TITLE_CHARS: usize = 80;
const MAX_SLUG_CHARS: usize = 48;

#[derive(Debug, Parser)]
#[command(name = "ca session", about = "Inspect and manage local sessions")]
pub struct SessionCliArgs {
    /// Profile whose sessions and recovery store are exported
    #[arg(long, default_value = "default")]
    profile: String,
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Export sessions and recovery state into a titled review pack
    ExportReview {
        /// Destination directory for the review pack
        #[arg(short, long, value_name = "DIR")]
        output: PathBuf,
        /// Export only this session (storage key, UUID, or public id; repeatable)
        #[arg(long = "session", value_name = "ID")]
        sessions: Vec<String>,
        /// Include archived sessions
        #[arg(long)]
        include_archived: bool,
        /// Replace a non-empty output directory
        #[arg(long)]
        overwrite: bool,
        /// Limit how many sessions are exported (most recently updated first)
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        /// Print machine-readable result JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ExportReviewRequest {
    pub sessions_dir: PathBuf,
    pub archived_sessions_dir: PathBuf,
    pub recovery_dir: PathBuf,
    pub output: PathBuf,
    pub only: Vec<String>,
    pub include_archived: bool,
    pub overwrite: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPackFile {
    pub relative_path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPackSession {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub workspace: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub archived: bool,
    pub has_recovery: bool,
    pub files: Vec<ReviewPackFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewPackResult {
    pub schema: String,
    pub output: String,
    pub session_count: usize,
    pub total_bytes: u64,
    pub sessions: Vec<ReviewPackSession>,
}

pub fn parse_args<I, T>(args: I) -> Result<SessionCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    SessionCliArgs::try_parse_from(args)
}

pub fn run_cli(cli: SessionCliArgs) -> i32 {
    let profile = match crate::runtime::ActiveProfile::parse(Some(&cli.profile)) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("ca session: {error}");
            return 2;
        }
    };
    let paths = match crate::runtime::RuntimePaths::discover() {
        Ok(paths) => paths.for_profile(&profile),
        Err(error) => {
            eprintln!("ca session: {error}");
            return 2;
        }
    };
    let SessionCommand::ExportReview {
        output,
        sessions,
        include_archived,
        overwrite,
        limit,
        json,
    } = cli.command;
    let recovery_dir = recovery_dir_for_sessions(&paths.sessions_dir);
    let request = ExportReviewRequest {
        sessions_dir: paths.sessions_dir,
        archived_sessions_dir: paths.archived_sessions_dir,
        recovery_dir,
        output,
        only: sessions,
        include_archived,
        overwrite,
        limit,
    };
    match run_export_review(request) {
        Ok(result) => {
            let rendered = if json {
                serde_json::to_string_pretty(&result).map(|value| format!("{value}\n"))
            } else {
                Ok(render_text(&result))
            };
            match rendered {
                Ok(text) => {
                    print!("{text}");
                    0
                }
                Err(error) => {
                    eprintln!("ca session export-review: {error}");
                    2
                }
            }
        }
        Err(error) => {
            eprintln!("ca session export-review: {error}");
            2
        }
    }
}

fn render_text(result: &ReviewPackResult) -> String {
    let mut out = format!(
        "Review pack created\nOutput: {}\nSessions: {}\nBytes: {}\n",
        result.output, result.session_count, result.total_bytes
    );
    for session in &result.sessions {
        out.push_str(&format!(
            "- {}  {}\n",
            session.folder,
            one_line_title(&session.title)
        ));
    }
    out
}

fn one_line_title(title: &str) -> String {
    let collapsed = title
        .chars()
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .collect::<String>();
    collapsed.trim().to_string()
}

fn recovery_dir_for_sessions(sessions_dir: &Path) -> PathBuf {
    sessions_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("recovery")
}

#[derive(Debug, Deserialize)]
struct TitleProbe {
    #[serde(default)]
    id: Option<Uuid>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    session: Option<Box<TitleProbe>>,
}

#[derive(Debug, Clone)]
struct SessionCandidate {
    storage_key: String,
    session_id: Option<Uuid>,
    metadata_path: Option<PathBuf>,
    archived: bool,
    title: String,
    workspace: Option<String>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn run_export_review(request: ExportReviewRequest) -> Result<ReviewPackResult, String> {
    let mut candidates = discover_candidates_in(&request.sessions_dir, false)?;
    if request.include_archived {
        candidates.extend(discover_candidates_in(
            &request.archived_sessions_dir,
            true,
        )?);
    }
    let known = candidates
        .iter()
        .filter_map(|candidate| candidate.session_id)
        .collect::<std::collections::BTreeSet<_>>();
    for recovery_id in recovery_session_ids(&request.recovery_dir)? {
        if known.contains(&recovery_id) {
            continue;
        }
        candidates.push(SessionCandidate {
            storage_key: recovery_id.to_string(),
            session_id: Some(recovery_id),
            metadata_path: None,
            archived: false,
            title: String::new(),
            workspace: None,
            created_at: None,
            updated_at: None,
        });
    }
    if !request.only.is_empty() {
        candidates.retain(|candidate| matches_filter(candidate, &request.only));
    }
    candidates.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.id_sort_key().cmp(&right.id_sort_key()))
    });
    if let Some(limit) = request.limit {
        candidates.truncate(limit);
    }
    if candidates.is_empty() {
        return Err("no sessions matched".into());
    }

    prepare_output_dir(&request.output, request.overwrite)?;
    let mut sessions = Vec::with_capacity(candidates.len());
    let mut total_bytes = 0_u64;
    for candidate in &candidates {
        let exported = export_one(&request, candidate)?;
        total_bytes += exported.files.iter().map(|file| file.bytes).sum::<u64>();
        sessions.push(exported);
    }

    let result = ReviewPackResult {
        schema: REVIEW_PACK_SCHEMA.into(),
        output: request.output.display().to_string(),
        session_count: sessions.len(),
        total_bytes,
        sessions,
    };
    write_pack_index(&request.output, &result)?;
    Ok(result)
}

impl SessionCandidate {
    fn id_sort_key(&self) -> String {
        self.session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| self.storage_key.clone())
    }

    fn display_id(&self) -> String {
        self.session_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| self.storage_key.clone())
    }
}

fn matches_filter(candidate: &SessionCandidate, only: &[String]) -> bool {
    let public = candidate
        .session_id
        .map(crate::session::public_session_id)
        .unwrap_or_default();
    only.iter().any(|filter| {
        filter.eq_ignore_ascii_case(&candidate.storage_key)
            || candidate
                .session_id
                .is_some_and(|id| id.to_string().eq_ignore_ascii_case(filter))
            || (!public.is_empty() && public.eq_ignore_ascii_case(filter))
    })
}

fn discover_candidates_in(
    sessions_dir: &Path,
    archived: bool,
) -> Result<Vec<SessionCandidate>, String> {
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(sessions_dir)
        .map_err(|error| format!("{}: {error}", sessions_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", sessions_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let storage_key = match path.file_stem().and_then(|value| value.to_str()) {
            Some(value) => value.to_string(),
            None => continue,
        };
        if storage_key.contains(".turns") {
            continue;
        }
        let probe = probe_title(&path);
        let session_id = probe
            .as_ref()
            .and_then(|probe| probe.id)
            .or_else(|| Uuid::parse_str(&storage_key).ok());
        let probe = probe.unwrap_or(TitleProbe {
            id: session_id,
            title: None,
            workspace: None,
            created_at: None,
            updated_at: None,
            session: None,
        });
        let nested = probe.session.as_deref();
        let title = nested
            .and_then(|inner| inner.title.clone())
            .or_else(|| probe.title.clone())
            .unwrap_or_default();
        let workspace = nested
            .and_then(|inner| inner.workspace.clone())
            .or_else(|| probe.workspace.clone());
        let created_at = nested
            .and_then(|inner| inner.created_at)
            .or(probe.created_at);
        let updated_at = nested
            .and_then(|inner| inner.updated_at)
            .or(probe.updated_at);
        candidates.push(SessionCandidate {
            storage_key,
            session_id,
            metadata_path: Some(path),
            archived,
            title,
            workspace,
            created_at,
            updated_at,
        });
    }
    Ok(candidates)
}

fn recovery_session_ids(recovery_dir: &Path) -> Result<Vec<Uuid>, String> {
    if !recovery_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    let entries = std::fs::read_dir(recovery_dir)
        .map_err(|error| format!("{}: {error}", recovery_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", recovery_dir.display()))?;
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Ok(id) = Uuid::parse_str(name) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Read only session metadata needed for titling. Never opens turn transcripts.
fn probe_title(path: &Path) -> Option<TitleProbe> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return None;
    }
    if metadata.len() > MAX_TITLE_PROBE_BYTES {
        return None;
    }
    let mut content = String::new();
    let file = std::fs::File::open(path).ok()?;
    {
        use std::io::Read;
        file.take(MAX_TITLE_PROBE_BYTES + 1)
            .read_to_string(&mut content)
            .ok()?;
    }
    if content.len() as u64 > MAX_TITLE_PROBE_BYTES {
        return None;
    }
    serde_json::from_str::<TitleProbe>(&content).ok()
}

fn prepare_output_dir(output: &Path, overwrite: bool) -> Result<(), String> {
    if output.exists() {
        let metadata = std::fs::symlink_metadata(output)
            .map_err(|error| format!("{}: {error}", output.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "export target is not a regular directory: {}",
                output.display()
            ));
        }
        let mut entries =
            std::fs::read_dir(output).map_err(|error| format!("{}: {error}", output.display()))?;
        let non_empty = entries.next().is_some();
        if non_empty {
            if !overwrite {
                return Err(format!(
                    "export target is not empty; pass --overwrite to replace it: {}",
                    output.display()
                ));
            }
            std::fs::remove_dir_all(output)
                .map_err(|error| format!("{}: {error}", output.display()))?;
        }
    }
    std::fs::create_dir_all(output).map_err(|error| format!("{}: {error}", output.display()))
}

fn export_one(
    request: &ExportReviewRequest,
    candidate: &SessionCandidate,
) -> Result<ReviewPackSession, String> {
    let title = bounded_title(&candidate.title);
    let folder_name = folder_name_for(&title, candidate);
    let folder = request.output.join(&folder_name);
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;

    let mut files = Vec::new();
    if let Some(metadata_path) = &candidate.metadata_path {
        let name = metadata_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("session.json")
            .to_string();
        let bytes = copy_file(metadata_path, &folder.join(&name))?;
        files.push(ReviewPackFile {
            relative_path: name,
            bytes,
        });
        for sibling in session_sidecar_files(metadata_path)? {
            let name = sibling
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            let bytes = copy_file(&sibling, &folder.join(&name))?;
            files.push(ReviewPackFile {
                relative_path: name,
                bytes,
            });
        }
    }

    let mut has_recovery = false;
    if let Some(session_id) = candidate.session_id {
        let recovery_src = request.recovery_dir.join(session_id.to_string());
        if recovery_src.is_dir() {
            has_recovery = true;
            let recovery_dst = folder.join("recovery");
            let copied = copy_tree(&recovery_src, &recovery_dst, "recovery")?;
            files.extend(copied);
        }
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let session = ReviewPackSession {
        id: candidate.display_id(),
        title,
        folder: folder_name,
        workspace: candidate.workspace.clone(),
        created_at: candidate
            .created_at
            .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        updated_at: candidate
            .updated_at
            .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        archived: candidate.archived,
        has_recovery,
        files,
    };
    let manifest_path = folder.join("manifest.json");
    let manifest = serde_json::to_string_pretty(&session)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    std::fs::write(&manifest_path, format!("{manifest}\n"))
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    Ok(session)
}

fn session_sidecar_files(metadata_path: &Path) -> Result<Vec<PathBuf>, String> {
    let parent = metadata_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let storage_key = metadata_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let mut sidecars = Vec::new();
    let entries =
        std::fs::read_dir(&parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", parent.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(storage_key) && name.ends_with(".turns.jsonl") {
            sidecars.push(path);
        }
    }
    sidecars.sort();
    Ok(sidecars)
}

fn copy_file(source: &Path, destination: &Path) -> Result<u64, String> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|error| format!("{}: {error}", source.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "refusing to copy non-regular session file: {}",
            source.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    std::fs::copy(source, destination).map_err(|error| format!("{}: {error}", source.display()))
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    relative_prefix: &str,
) -> Result<Vec<ReviewPackFile>, String> {
    let mut copied = Vec::new();
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("{}: {error}", destination.display()))?;
    let entries =
        std::fs::read_dir(source).map_err(|error| format!("{}: {error}", source.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", source.display()))?;
        names.push(entry.file_name());
    }
    names.sort();
    for name in names {
        let from = source.join(&name);
        let to = destination.join(&name);
        let metadata = std::fs::symlink_metadata(&from)
            .map_err(|error| format!("{}: {error}", from.display()))?;
        let relative = format!("{relative_prefix}/{}", name.to_string_lossy());
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing to copy recovery symlink: {}",
                from.display()
            ));
        }
        if metadata.is_dir() {
            copied.extend(copy_tree(&from, &to, &relative)?);
        } else if metadata.is_file() {
            let bytes = copy_file(&from, &to)?;
            copied.push(ReviewPackFile {
                relative_path: relative,
                bytes,
            });
        }
    }
    Ok(copied)
}

fn bounded_title(title: &str) -> String {
    let collapsed = title
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    let collapsed = collapsed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "Untitled session".into();
    }
    collapsed.chars().take(MAX_TITLE_CHARS).collect()
}

fn folder_name_for(title: &str, candidate: &SessionCandidate) -> String {
    let slug = title_slug(title);
    let id = candidate
        .session_id
        .map(|id| short_id(&id))
        .unwrap_or_else(|| short_key(&candidate.storage_key));
    if slug.is_empty() {
        format!("untitled--{id}")
    } else {
        format!("{slug}--{id}")
    }
}

fn short_id(id: &Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

fn short_key(key: &str) -> String {
    let cleaned = key
        .chars()
        .filter(|ch| ch.is_ascii_hexdigit())
        .take(8)
        .collect::<String>();
    if cleaned.len() == 8 {
        cleaned
    } else {
        let fallback = key
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '0' })
            .take(8)
            .collect::<String>();
        if fallback.len() == 8 {
            fallback
        } else {
            format!("{key:0<8}")
        }
    }
}

fn title_slug(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in title.chars() {
        if slug.chars().count() >= MAX_SLUG_CHARS {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch);
        } else {
            pending_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn write_pack_index(output: &Path, result: &ReviewPackResult) -> Result<(), String> {
    let manifest_path = output.join("manifest.json");
    let manifest = serde_json::to_string_pretty(result)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;
    std::fs::write(&manifest_path, format!("{manifest}\n"))
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;

    let mut index = format!(
        "# Session review pack\n\n- Schema: `{}`\n- Sessions: {}\n- Bytes: {}\n\n",
        result.schema, result.session_count, result.total_bytes
    );
    for session in &result.sessions {
        index.push_str(&format!(
            "## {}\n\n- Folder: `{}`\n- Session: `{}`\n- Title: {}\n",
            session.title, session.folder, session.id, session.title
        ));
        if let Some(workspace) = &session.workspace {
            index.push_str(&format!("- Workspace: `{workspace}`\n"));
        }
        if let Some(updated_at) = &session.updated_at {
            index.push_str(&format!("- Updated: {updated_at}\n"));
        }
        index.push_str(&format!(
            "- Recovery: {}\n",
            if session.has_recovery {
                "included"
            } else {
                "none"
            }
        ));
        index.push_str(&format!("- Files: {}\n\n", session.files.len()));
    }
    let index_path = output.join("index.md");
    std::fs::write(&index_path, index).map_err(|error| format!("{}: {error}", index_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_session_bundle(
        sessions_dir: &Path,
        id: Uuid,
        title: &str,
        turns_body: &str,
    ) -> PathBuf {
        write_session_bundle_at(sessions_dir, id, title, turns_body, "2026-10-11T01:00:00Z")
    }

    fn write_session_bundle_at(
        sessions_dir: &Path,
        id: Uuid,
        title: &str,
        turns_body: &str,
        updated_at: &str,
    ) -> PathBuf {
        std::fs::create_dir_all(sessions_dir).unwrap();
        let storage_key = id.to_string();
        let metadata = serde_json::json!({
            "storage_version": 1,
            "storage_key": storage_key,
            "id": storage_key,
            "generation": "11111111-1111-1111-1111-111111111111",
            "message_count": 1,
            "committed_bytes": turns_body.len(),
            "offsets": [0],
            "history_sha256": "0".repeat(64),
            "session": {
                "version": 4,
                "id": storage_key,
                "title": title,
                "workspace": "/tmp/workspace",
                "created_at": "2026-10-11T00:00:00Z",
                "updated_at": updated_at,
                "provider": "fake",
                "model": "fake-model",
                "messages": [{"role": "user", "content": "should-not-be-titled-from-this"}]
            }
        });
        let metadata_path = sessions_dir.join(format!("{storage_key}.json"));
        std::fs::write(&metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        std::fs::write(
            sessions_dir.join(format!(
                "{storage_key}.11111111-1111-1111-1111-111111111111.turns.jsonl"
            )),
            turns_body,
        )
        .unwrap();
        metadata_path
    }

    fn write_recovery(recovery_dir: &Path, id: Uuid) {
        let root = recovery_dir.join(id.to_string());
        std::fs::create_dir_all(root.join("blobs")).unwrap();
        std::fs::write(
            root.join("journal.json"),
            r#"{"version":1,"session_id":"x","revision":1,"groups":[]}"#,
        )
        .unwrap();
        std::fs::write(root.join("blobs").join("aa"), "blob-bytes").unwrap();
    }

    #[test]
    fn title_probe_reads_metadata_only_and_ignores_turns() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let id = Uuid::new_v4();
        write_session_bundle(&sessions, id, "Review Current State", "SECRET TURN BODY\n");
        let path = sessions.join(format!("{id}.json"));
        let probe = probe_title(&path).unwrap();
        assert_eq!(probe.id, Some(id));
        let nested = probe.session.unwrap();
        assert_eq!(nested.title.as_deref(), Some("Review Current State"));
        let turns = sessions.join(format!(
            "{id}.11111111-1111-1111-1111-111111111111.turns.jsonl"
        ));
        assert!(std::fs::read_to_string(turns).unwrap().contains("SECRET"));
    }

    #[test]
    fn export_review_builds_titled_pack_without_parsing_turns() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let recovery = root.path().join("recovery");
        let output = root.path().join("out");
        let id = Uuid::new_v4();
        write_session_bundle(
            &sessions,
            id,
            "Check paused Transmission torrents on BSD",
            "line one\nline two\n",
        );
        write_recovery(&recovery, id);

        let result = run_export_review(ExportReviewRequest {
            sessions_dir: sessions.clone(),
            archived_sessions_dir: root.path().join("archive"),
            recovery_dir: recovery.clone(),
            output: output.clone(),
            only: Vec::new(),
            include_archived: false,
            overwrite: false,
            limit: None,
        })
        .unwrap();

        assert_eq!(result.session_count, 1);
        let session = &result.sessions[0];
        assert_eq!(session.title, "Check paused Transmission torrents on BSD");
        assert!(session.has_recovery);
        assert!(session
            .folder
            .starts_with("check-paused-transmission-torrents-on-bsd--"));
        let pack = output.join(&session.folder);
        assert!(pack.join("manifest.json").is_file());
        assert!(pack
            .join(format!(
                "{id}.11111111-1111-1111-1111-111111111111.turns.jsonl"
            ))
            .is_file());
        assert!(pack.join("recovery/journal.json").is_file());
        assert!(pack.join("recovery/blobs/aa").is_file());
        assert!(output.join("manifest.json").is_file());
        assert!(output.join("index.md").is_file());
    }

    #[test]
    fn export_review_requires_overwrite_for_non_empty_target() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let output = root.path().join("out");
        let id = Uuid::new_v4();
        write_session_bundle(&sessions, id, "Alpha", "x\n");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("existing.txt"), "keep").unwrap();

        let request = ExportReviewRequest {
            sessions_dir: sessions,
            archived_sessions_dir: root.path().join("archive"),
            recovery_dir: root.path().join("recovery"),
            output: output.clone(),
            only: Vec::new(),
            include_archived: false,
            overwrite: false,
            limit: None,
        };
        let error = run_export_review(request.clone()).unwrap_err();
        assert!(error.contains("--overwrite"));
        assert!(output.join("existing.txt").is_file());

        let request = ExportReviewRequest {
            overwrite: true,
            ..request
        };
        let result = run_export_review(request).unwrap();
        assert_eq!(result.session_count, 1);
        assert!(!output.join("existing.txt").exists());
    }

    #[test]
    fn export_review_filters_and_limits() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let output = root.path().join("out");
        let older = Uuid::new_v4();
        let newer = Uuid::new_v4();
        write_session_bundle_at(
            &sessions,
            older,
            "Older session",
            "a\n",
            "2026-10-11T00:00:00Z",
        );
        write_session_bundle_at(
            &sessions,
            newer,
            "Newer session",
            "b\n",
            "2026-10-11T02:00:00Z",
        );

        let result = run_export_review(ExportReviewRequest {
            sessions_dir: sessions.clone(),
            archived_sessions_dir: root.path().join("archive"),
            recovery_dir: root.path().join("recovery"),
            output: output.clone(),
            only: vec![newer.to_string()],
            include_archived: false,
            overwrite: false,
            limit: None,
        })
        .unwrap();
        assert_eq!(result.session_count, 1);
        assert_eq!(result.sessions[0].id, newer.to_string());

        let result = run_export_review(ExportReviewRequest {
            sessions_dir: sessions,
            archived_sessions_dir: root.path().join("archive"),
            recovery_dir: root.path().join("recovery"),
            output: root.path().join("out2"),
            only: Vec::new(),
            include_archived: false,
            overwrite: false,
            limit: Some(1),
        })
        .unwrap();
        assert_eq!(result.session_count, 1);
        assert_eq!(result.sessions[0].id, newer.to_string());
    }

    #[test]
    fn recovery_only_sessions_are_exported() {
        let root = tempfile::tempdir().unwrap();
        let recovery = root.path().join("recovery");
        let id = Uuid::new_v4();
        write_recovery(&recovery, id);
        let result = run_export_review(ExportReviewRequest {
            sessions_dir: root.path().join("sessions"),
            archived_sessions_dir: root.path().join("archive"),
            recovery_dir: recovery,
            output: root.path().join("out"),
            only: Vec::new(),
            include_archived: false,
            overwrite: false,
            limit: None,
        })
        .unwrap();
        assert_eq!(result.session_count, 1);
        assert_eq!(result.sessions[0].title, "Untitled session");
        assert!(result.sessions[0].has_recovery);
    }

    #[test]
    fn parse_args_accepts_export_review() {
        let cli = parse_args([
            "ca session",
            "export-review",
            "--output",
            "/tmp pack",
            "--session",
            "18dd4890-6b15-45f8-aed2-93569584bd90",
            "--limit",
            "3",
            "--json",
        ])
        .unwrap();
        assert_eq!(cli.profile, "default");
        let SessionCommand::ExportReview {
            output,
            sessions,
            limit,
            json,
            include_archived,
            overwrite,
        } = cli.command;
        assert_eq!(output, PathBuf::from("/tmp pack"));
        assert_eq!(
            sessions,
            vec!["18dd4890-6b15-45f8-aed2-93569584bd90".to_string()]
        );
        assert_eq!(limit, Some(3));
        assert!(json);
        assert!(!include_archived);
        assert!(!overwrite);
    }

    #[test]
    fn folder_names_are_filesystem_safe() {
        let candidate = SessionCandidate {
            storage_key: Uuid::new_v4().to_string(),
            session_id: Some(Uuid::new_v4()),
            metadata_path: None,
            archived: false,
            title: String::new(),
            workspace: None,
            created_at: None,
            updated_at: None,
        };
        let name = folder_name_for("My request: /tmp/path?*", &candidate);
        assert!(name.starts_with("my-request-tmp-path--"));
        assert!(!name.contains('/'));
        assert!(!name.contains('?'));
    }
}
