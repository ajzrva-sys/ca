//! Repository snapshots.
//!
//! Target home for before/after file fingerprints used to prove what Worker
//! changed without trusting prose.

use std::collections::HashMap;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::app::relative_slash_path;
use crate::config::Config;
use crate::paths::Paths;

use super::glob::compile_globs;
use super::report::FileFingerprint;

pub(crate) fn snapshot_repo(
    paths: &Paths,
    cfg: &Config,
) -> Result<HashMap<String, FileFingerprint>> {
    if !cfg.guard.enforce_worker_allowed_files {
        return Ok(HashMap::new());
    }

    let ignored = compile_globs(&cfg.guard.ignored_paths)?;
    let mut files = HashMap::new();

    for entry in WalkDir::new(&paths.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            relative_slash_path(&paths.root, entry.path())
                .map(|rel| !ignored.is_match(rel))
                .unwrap_or(true)
        })
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = relative_slash_path(&paths.root, entry.path()) else {
            continue;
        };
        if ignored.is_match(&rel) {
            continue;
        }
        files.insert(rel, fingerprint_file(entry.path())?);
    }

    Ok(files)
}

pub(crate) fn snapshot_control_plane(paths: &Paths) -> Result<HashMap<String, FileFingerprint>> {
    let mut files = HashMap::new();
    let candidates = [
        paths.config.clone(),
        paths.state.clone(),
        paths.lock.clone(),
        paths.prompts.clone(),
        paths.schemas.clone(),
        paths.orch.join("imports"),
        paths.orch.join("pending-commit-recovery.json"),
        paths.orch.join("mcp").join("approval-required.json"),
    ];
    for path in &candidates {
        if path.is_file() {
            if let Some(rel) = relative_slash_path(&paths.root, path) {
                files.insert(rel, fingerprint_file(path)?);
            }
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        for entry in WalkDir::new(path).follow_links(false) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(rel) = relative_slash_path(&paths.root, entry.path()) {
                files.insert(rel, fingerprint_file(entry.path())?);
            }
        }
    }
    Ok(files)
}

pub(crate) fn fingerprint_file(path: &Path) -> Result<FileFingerprint> {
    let metadata = fs::metadata(path)?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    let mut file = File::open(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buf = [0_u8; 16 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        buf[..n].hash(&mut hasher);
    }

    Ok(FileFingerprint {
        len: metadata.len(),
        modified_ns,
        content_hash: hasher.finish(),
    })
}
