//! Bounded host snapshots for shell effects, including promoted/background jobs.
//!
//! Volume metadata such as `.fseventsd` is not listable. Those directories are
//! skipped instead of failing every command. The content budget is spent on
//! smaller files first so a large binary does not push source edits out of the
//! snapshot. Files that still do not fit keep a fingerprint. A later change to
//! one of those files does not hold completion; bounded text that can be read
//! is still recorded.
use crate::undo::{FileMutationBatchReceipt, FileMutationReceipt, FileState};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::UNIX_EPOCH;
const MAX_CAPTURE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CAPTURE_FILES: usize = 20_000;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    mtime_secs: i64,
    mtime_nanos: u32,
    inode: u64,
}
#[derive(Clone)]
struct Capture {
    turn: Option<uuid::Uuid>,
    roots: Vec<PathBuf>,
    files: Arc<BTreeMap<PathBuf, String>>,
    fingerprints: Arc<BTreeMap<PathBuf, Fingerprint>>,
    started_at: chrono::DateTime<chrono::Utc>,
}
static CAPTURES: LazyLock<Mutex<BTreeMap<uuid::Uuid, Capture>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static TURNS: LazyLock<Mutex<BTreeMap<uuid::Uuid, uuid::Uuid>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
type OwnerLocks = BTreeMap<uuid::Uuid, Weak<tokio::sync::Mutex<()>>>;
static OWNER_LOCKS: LazyLock<Mutex<OwnerLocks>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn owner_lock(owner: crate::jobs::JobOwner) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = OWNER_LOCKS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&owner.run_id).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(owner.run_id, Arc::downgrade(&lock));
    lock
}

pub(crate) fn bind_turn(owner: crate::jobs::JobOwner, turn: uuid::Uuid) {
    TURNS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(owner.run_id, turn);
}
fn eligible(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    !matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "pdf"
            | "mp3"
            | "mp4"
            | "zip"
            | "gz"
            | "bz2"
            | "xz"
            | "dmg"
            | "a"
            | "o"
            | "dylib"
            | "so"
            | "sqlite"
            | "db"
            | "log"
    )
}
fn skipped_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str().unwrap_or(""),
        ".git"
            | "target"
            | "node_modules"
            | "scratch"
            | "oracles"
            | "reference"
            | ".venv"
            | "__pycache__"
            | ".DocumentRevisions-V100"
            | ".fseventsd"
            | ".Spotlight-V100"
            | ".TemporaryItems"
            | ".Trashes"
    )
}
fn included(entry: &ignore::DirEntry) -> bool {
    entry.depth() == 0 || !skipped_directory(entry.file_name())
}
fn io_unreadable(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound
    )
}
fn enumeration_skippable(error: &ignore::Error) -> bool {
    error.io_error().is_some_and(io_unreadable)
}
fn fingerprint(metadata: &std::fs::Metadata) -> Fingerprint {
    let (mtime_secs, mtime_nanos) = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs() as i64, duration.subsec_nanos()))
        .unwrap_or((0, 0));
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(metadata);
    #[cfg(not(unix))]
    let inode = 0;
    Fingerprint {
        len: metadata.len(),
        mtime_secs,
        mtime_nanos,
        inode,
    }
}
fn over_content_budget(bytes: usize, files: usize, length: u64) -> bool {
    length > MAX_CAPTURE_BYTES as u64
        || files >= MAX_CAPTURE_FILES
        || usize::try_from(length)
            .map(|length| bytes.saturating_add(length) > MAX_CAPTURE_BYTES)
            .unwrap_or(true)
}
fn read_tracked_utf8(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > crate::undo::MAX_UNDO_FILE_BYTES as u64 {
        return None;
    }
    String::from_utf8(std::fs::read(path).ok()?).ok()
}
struct Snapshot {
    files: BTreeMap<PathBuf, String>,
    fingerprints: BTreeMap<PathBuf, Fingerprint>,
}
struct SeenFile {
    path: PathBuf,
    mark: Fingerprint,
    len: u64,
}
fn scan(roots: &[PathBuf]) -> Result<Snapshot, String> {
    let mut seen = Vec::new();
    for root in roots {
        match std::fs::read_dir(root) {
            Ok(entries) => drop(entries),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                return Err("shell mutation evidence could not enumerate the workspace".into());
            }
        }
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .parents(false)
            .git_ignore(false)
            .git_exclude(false)
            .git_global(false)
            .ignore(false)
            .follow_links(false)
            .filter_entry(included)
            .build();
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if enumeration_skippable(&error) => continue,
                Err(_) => {
                    return Err("shell mutation evidence could not enumerate the workspace".into());
                }
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) || !eligible(entry.path()) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.io_error().is_some_and(io_unreadable) => continue,
                Err(_) => {
                    return Err("shell mutation evidence could not inspect a workspace file".into());
                }
            };
            seen.push(SeenFile {
                path: entry.path().to_path_buf(),
                mark: fingerprint(&metadata),
                len: metadata.len(),
            });
        }
    }
    // A large binary must not consume the budget ahead of the files a turn
    // can actually review. Equal sizes stay path-ordered so the snapshot is stable.
    seen.sort_by(|left, right| left.len.cmp(&right.len).then(left.path.cmp(&right.path)));
    let mut files = BTreeMap::new();
    let mut fingerprints = BTreeMap::new();
    let mut bytes = 0usize;
    for item in seen {
        if over_content_budget(bytes, files.len(), item.len) {
            fingerprints.insert(item.path, item.mark);
            continue;
        }
        let content = match std::fs::read(&item.path) {
            Ok(content) => content,
            Err(error) if io_unreadable(&error) => {
                fingerprints.insert(item.path, item.mark);
                continue;
            }
            Err(_) => {
                return Err("shell mutation evidence could not read a workspace file".into());
            }
        };
        let Ok(content) = String::from_utf8(content) else {
            continue;
        };
        bytes += content.len();
        fingerprints.insert(item.path.clone(), item.mark);
        files.insert(item.path, content);
    }
    Ok(Snapshot {
        files,
        fingerprints,
    })
}
fn receipt(path: &Path, before: FileState, after: FileState) -> FileMutationReceipt {
    FileMutationReceipt::new(path.to_path_buf(), "run_command", before, after)
}
fn diff(before: &Capture, after: &Snapshot) -> Result<Vec<FileMutationReceipt>, String> {
    let mut paths = before
        .fingerprints
        .keys()
        .chain(after.fingerprints.keys())
        .chain(before.files.keys())
        .chain(after.files.keys())
        .cloned()
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut receipts = Vec::new();
    for path in paths {
        let before_fp = before.fingerprints.get(&path);
        let after_fp = after.fingerprints.get(&path);
        let before_text = before.files.get(&path);
        let after_text = after.files.get(&path);
        if before_fp.is_some() && before_fp == after_fp {
            if let (Some(before_text), Some(after_text)) = (before_text, after_text) {
                if before_text != after_text {
                    receipts.push(receipt(
                        &path,
                        FileState::Utf8(before_text.clone()),
                        FileState::Utf8(after_text.clone()),
                    ));
                }
            }
            continue;
        }
        match (before_text, after_text) {
            (Some(before_text), Some(after_text)) => {
                if before_text != after_text {
                    receipts.push(receipt(
                        &path,
                        FileState::Utf8(before_text.clone()),
                        FileState::Utf8(after_text.clone()),
                    ));
                }
            }
            (None, Some(after_text)) => {
                // The previous bytes were outside the capture. Review still sees
                // the bounded text that exists now.
                receipts.push(receipt(
                    &path,
                    FileState::Missing,
                    FileState::Utf8(after_text.clone()),
                ));
            }
            (Some(before_text), None) if after_fp.is_none() => {
                receipts.push(receipt(
                    &path,
                    FileState::Utf8(before_text.clone()),
                    FileState::Missing,
                ));
            }
            (Some(before_text), None) => match read_tracked_utf8(&path) {
                Some(after_text) if after_text == *before_text => {}
                Some(after_text) => receipts.push(receipt(
                    &path,
                    FileState::Utf8(before_text.clone()),
                    FileState::Utf8(after_text),
                )),
                // Still present, but no longer bounded text. Do not hold the turn.
                None => {}
            },
            (None, None) if before_fp.is_none() && after_fp.is_some() => {
                if let Some(after_text) = read_tracked_utf8(&path) {
                    receipts.push(receipt(
                        &path,
                        FileState::Missing,
                        FileState::Utf8(after_text),
                    ));
                }
            }
            (None, None) => {}
        }
    }
    Ok(receipts)
}
pub(crate) fn has_pending(owner: crate::jobs::JobOwner) -> bool {
    CAPTURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .contains_key(&owner.run_id)
}
pub(crate) fn started_at(owner: crate::jobs::JobOwner) -> Option<chrono::DateTime<chrono::Utc>> {
    CAPTURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&owner.run_id)
        .map(|capture| capture.started_at)
}
pub(crate) async fn begin(owner: crate::jobs::JobOwner, roots: &[PathBuf]) -> Result<(), String> {
    let lock = owner_lock(owner);
    let _guard = lock.lock().await;
    let turn = TURNS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&owner.run_id)
        .copied();
    let previous = CAPTURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&owner.run_id)
        .map(|capture| capture.turn);
    if previous == Some(turn) {
        return Ok(());
    }
    if previous.is_some() {
        // A fresh human request starts from the settled workspace, including
        // changes left by a cancelled task. Same-task retries retain the capture.
        finish_inner(owner, true).await?;
    }
    let started_at = chrono::Utc::now();
    let roots = roots.to_vec();
    let snapshot_roots = roots.clone();
    let snapshot = tokio::task::spawn_blocking(move || scan(&snapshot_roots))
        .await
        .map_err(|_| "shell evidence worker stopped")??;
    CAPTURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .entry(owner.run_id)
        .or_insert(Capture {
            turn,
            roots,
            files: Arc::new(snapshot.files),
            fingerprints: Arc::new(snapshot.fingerprints),
            started_at,
        });
    Ok(())
}
pub(crate) async fn finish(
    owner: crate::jobs::JobOwner,
) -> Result<Vec<FileMutationBatchReceipt>, String> {
    let lock = owner_lock(owner);
    let _guard = lock.lock().await;
    finish_inner(owner, true).await
}

pub(crate) async fn preview(
    owner: crate::jobs::JobOwner,
) -> Result<Vec<FileMutationBatchReceipt>, String> {
    let lock = owner_lock(owner);
    let _guard = lock.lock().await;
    finish_inner(owner, false).await
}

async fn finish_inner(
    owner: crate::jobs::JobOwner,
    consume: bool,
) -> Result<Vec<FileMutationBatchReceipt>, String> {
    let Some(capture) = CAPTURES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&owner.run_id)
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let settle = async {
        loop {
            if crate::jobs::JobManager::process()
                .list_owned(owner)
                .await
                .iter()
                .all(|job| {
                    !matches!(
                        job.status,
                        crate::jobs::JobStatus::Running | crate::jobs::JobStatus::Cancelling
                    )
                })
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(1800), settle)
        .await
        .map_err(|_| "independent review is waiting for unfinished shell writers")?;
    let current_turn = TURNS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&owner.run_id)
        .copied();
    if capture.turn != current_turn {
        // A new task can finish without admitting another shell call. Its
        // final barrier must settle earlier writers without claiming their edits.
        if consume {
            CAPTURES
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&owner.run_id);
        }
        return Ok(Vec::new());
    }
    let roots = capture.roots.clone();
    let after = tokio::task::spawn_blocking(move || scan(&roots))
        .await
        .map_err(|_| "shell evidence worker stopped")??;
    let receipts = diff(&capture, &after)?;
    let batches = receipts
        .chunks(crate::undo::MAX_MUTATION_BATCH_FILES)
        .map(|chunk| FileMutationBatchReceipt::new(chunk.to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    if consume {
        CAPTURES
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&owner.run_id);
    }
    Ok(batches)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct RestoreMode(PathBuf);

    #[cfg(unix)]
    impl Drop for RestoreMode {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    #[tokio::test]
    async fn abandoned_review_preview_preserves_original_shell_capture_for_retry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "original bytes").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        bind_turn(owner, uuid::Uuid::new_v4());
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "first candidate").unwrap();
        let first = preview(owner).await.unwrap();
        assert!(has_pending(owner));
        let repeated = preview(owner).await.unwrap();
        assert_eq!(first.len(), repeated.len());
        assert_eq!(first[0].digest, repeated[0].digest);
        std::fs::write(&path, "repaired candidate").unwrap();
        let retry = preview(owner).await.unwrap();
        assert_eq!(
            retry[0].receipts()[0].before,
            FileState::Utf8("original bytes".into())
        );
        assert_eq!(
            retry[0].receipts()[0].after,
            FileState::Utf8("repaired candidate".into())
        );
        assert!(has_pending(owner));
        let published = finish(owner).await.unwrap();
        assert_eq!(published.len(), retry.len());
        assert_eq!(published[0].digest, retry[0].digest);
        assert!(!has_pending(owner));
    }
    #[tokio::test]
    async fn fresh_turn_rebases_once_under_parallel_shell_admission() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "before cancelled task").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let old_turn = uuid::Uuid::new_v4();
        bind_turn(owner, old_turn);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "cancelled task bytes").unwrap();
        // A retry still owns the earlier baseline; a new human turn owns only
        // changes after the settled bytes, even when shell calls start together.
        bind_turn(owner, old_turn);
        begin(owner, &[root.path().into()]).await.unwrap();
        assert_eq!(
            CAPTURES.lock().unwrap().get(&owner.run_id).unwrap().files[&path],
            "before cancelled task"
        );
        let new_turn = uuid::Uuid::new_v4();
        bind_turn(owner, new_turn);
        let roots = [root.path().to_path_buf()];
        let (first, second) = tokio::join!(begin(owner, &roots), begin(owner, &roots));
        first.unwrap();
        second.unwrap();
        assert!(has_pending(owner));
        std::fs::write(&path, "new task bytes").unwrap();
        let batches = finish(owner).await.unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(
            batches[0].receipts()[0].before,
            FileState::Utf8("cancelled task bytes".into())
        );
        assert_eq!(
            batches[0].receipts()[0].after,
            FileState::Utf8("new task bytes".into())
        );
    }
    #[tokio::test]
    async fn fresh_turn_finalization_does_not_claim_cancelled_task_shell_edits() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "before cancelled task").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        bind_turn(owner, uuid::Uuid::new_v4());
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "cancelled task bytes").unwrap();
        bind_turn(owner, uuid::Uuid::new_v4());
        assert!(finish(owner).await.unwrap().is_empty());
        assert!(!has_pending(owner));
    }
    #[tokio::test]
    async fn dirty_baseline_creation_deletion_and_noop_are_distinct() {
        let root = tempfile::tempdir().unwrap();
        let existing = root.path().join("source.rs");
        std::fs::write(&existing, "pre-existing dirty bytes").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&existing, "changed").unwrap();
        std::fs::write(root.path().join("test.rs"), "new").unwrap();
        let batches = finish(owner).await.unwrap();
        assert_eq!(batches.iter().flat_map(|batch| batch.receipts()).count(), 2);
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.before == FileState::Utf8("pre-existing dirty bytes".into())));
        begin(owner, &[root.path().into()]).await.unwrap();
        assert!(finish(owner).await.unwrap().is_empty());
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::remove_file(&existing).unwrap();
        assert!(finish(owner)
            .await
            .unwrap()
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.after == FileState::Missing));
    }

    #[tokio::test]
    async fn fresh_tasks_rebase_but_retries_retain_their_original_capture() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.rs");
        std::fs::write(&path, "initial").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let first = uuid::Uuid::new_v4();
        bind_turn(owner, first);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "interrupted edit").unwrap();
        bind_turn(owner, first);
        begin(owner, &[root.path().into()]).await.unwrap();
        let retry = finish(owner).await.unwrap();
        assert_eq!(
            retry[0].receipts()[0].before,
            FileState::Utf8("initial".into())
        );
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "previous task dirt").unwrap();
        bind_turn(owner, uuid::Uuid::new_v4());
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&path, "new task edit").unwrap();
        let fresh = finish(owner).await.unwrap();
        assert_eq!(
            fresh[0].receipts()[0].before,
            FileState::Utf8("previous task dirt".into())
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unlistable_child_does_not_block_the_baseline() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let sealed = root.path().join("sealed");
        std::fs::create_dir(&sealed).unwrap();
        std::fs::write(sealed.join("hidden.txt"), "secret").unwrap();
        let _restore = RestoreMode(sealed.clone());
        let mut permissions = std::fs::metadata(&sealed).unwrap().permissions();
        permissions.set_mode(0o111);
        std::fs::set_permissions(&sealed, permissions).unwrap();
        let visible = root.path().join("source.rs");
        std::fs::write(&visible, "visible").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&visible, "edited").unwrap();
        let mut permissions = std::fs::metadata(&sealed).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&sealed, permissions).unwrap();
        std::fs::write(sealed.join("hidden.txt"), "edited secret").unwrap();
        let mut permissions = std::fs::metadata(&sealed).unwrap().permissions();
        permissions.set_mode(0o111);
        std::fs::set_permissions(&sealed, permissions).unwrap();
        let batches = finish(owner).await.unwrap();
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.path == visible && file.after == FileState::Utf8("edited".into())));
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .all(|file| !file.path.starts_with(&sealed)));
    }

    #[tokio::test]
    async fn volume_metadata_directories_are_not_part_of_the_baseline() {
        let root = tempfile::tempdir().unwrap();
        let events = root.path().join(".fseventsd");
        std::fs::create_dir(&events).unwrap();
        std::fs::write(events.join("note.txt"), "event").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(events.join("note.txt"), "changed").unwrap();
        assert!(finish(owner).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn oversized_file_does_not_block_an_unrelated_edit() {
        let root = tempfile::tempdir().unwrap();
        let oversized = root.path().join("oversized.txt");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        let marker = root.path().join("marker.txt");
        std::fs::write(&marker, "created").unwrap();
        let batches = finish(owner).await.unwrap();
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.path == marker && file.before == FileState::Missing));
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .all(|file| file.path != oversized));
    }

    #[tokio::test]
    async fn changing_an_oversized_file_does_not_hold_completion() {
        let root = tempfile::tempdir().unwrap();
        let oversized = root.path().join("oversized.txt");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&oversized, "replaced").unwrap();
        let kept = root.path().join("kept.rs");
        std::fs::write(&kept, "still reviewed").unwrap();
        let batches = finish(owner).await.unwrap();
        let receipts = batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .collect::<Vec<_>>();
        assert!(receipts.iter().any(|file| {
            file.path == oversized && file.after == FileState::Utf8("replaced".into())
        }));
        assert!(receipts.iter().any(
            |file| file.path == kept && file.after == FileState::Utf8("still reviewed".into())
        ));
    }

    #[tokio::test]
    async fn oversized_metadata_change_does_not_hold_a_source_edit() {
        let root = tempfile::tempdir().unwrap();
        let oversized = root.path().join("oversized.bin");
        std::fs::File::create(&oversized)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        let source = root.path().join("source.rs");
        std::fs::write(&source, "before").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::File::options()
            .write(true)
            .open(&oversized)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 8)
            .unwrap();
        std::fs::write(&source, "after").unwrap();
        let batches = finish(owner).await.unwrap();
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| file.path == source && file.after == FileState::Utf8("after".into())));
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .all(|file| file.path != oversized));
    }

    #[tokio::test]
    async fn small_source_edit_stays_captured_when_large_files_fill_the_budget() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a-large.txt", "b-large.txt"] {
            std::fs::File::create(root.path().join(name))
                .unwrap()
                .set_len(40 * 1024 * 1024)
                .unwrap();
        }
        let source = root.path().join("z-source.rs");
        std::fs::write(&source, "before").unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        begin(owner, &[root.path().into()]).await.unwrap();
        std::fs::write(&source, "after").unwrap();
        let batches = finish(owner).await.unwrap();
        assert!(batches
            .iter()
            .flat_map(|batch| batch.receipts())
            .any(|file| {
                file.path == source
                    && file.before == FileState::Utf8("before".into())
                    && file.after == FileState::Utf8("after".into())
            }));
    }
}
