//! Project policy commits compare the complete authoritative rule snapshot.
//! Call these bounded filesystem operations on a worker.

use super::policy::Policy;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

const MAX_POLICY_BYTES: usize = 1024 * 1024;

pub(super) fn load(path: &Path) -> Result<Policy, String> {
    check_owned_parents(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_POLICY_BYTES as u64 => {}
        Ok(_) => return Err("Privacy rules exceed their regular-file bound".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Policy::default());
        }
        Err(_) => return Err("Project privacy rules cannot be inspected safely".into()),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("Privacy rules changed while being opened; reload them".into());
        }
        Err(_) => return Err("Project privacy rules cannot be read safely".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Privacy rule metadata is unavailable")?;
    if !metadata.is_file() || metadata.len() > MAX_POLICY_BYTES as u64 {
        return Err("Privacy rules exceed their regular-file bound".into());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_POLICY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Privacy rules could not be read completely")?;
    if bytes.len() != metadata.len() as usize || bytes.len() > MAX_POLICY_BYTES {
        return Err("Privacy rules changed while being read; reload them".into());
    }
    let policy: Policy = serde_json::from_slice(&bytes)
        .map_err(|_| "Project privacy rules are invalid; no request was sent")?;
    policy.validate()?;
    Ok(policy)
}

pub(super) fn save(path: &Path, expected: &Policy, draft: &Policy) -> Result<Policy, String> {
    save_inner(path, expected, draft, &mut |_| Ok(()))
}

pub(super) fn load_task_selection(path: &Path) -> Result<crate::task_filter::ProjectEdits, String> {
    check_owned_parents(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(_) => return Err("Remembered filter edits cannot be read safely.".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Remembered filter metadata is unavailable")?;
    if !metadata.is_file() || metadata.len() > MAX_POLICY_BYTES as u64 {
        return Err("Remembered filter edits exceed their regular-file bound.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_POLICY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Remembered filter edits could not be read")?;
    if bytes.len() != metadata.len() as usize || bytes.len() > MAX_POLICY_BYTES {
        return Err("Remembered filter edits changed while reading.".into());
    }
    let edits: crate::task_filter::ProjectEdits =
        serde_json::from_slice(&bytes).map_err(|_| "Remembered filter edits are invalid")?;
    edits.validate()?;
    Ok(edits)
}

pub(super) fn remember_task_selection(
    path: &Path,
    suggested: &crate::task_filter::Policy,
    selected: &crate::task_filter::Policy,
) -> Result<(), String> {
    suggested.validate()?;
    selected.validate()?;
    check_owned_parents(path)?;
    let parent = path
        .parent()
        .ok_or("Remembered filter directory is unavailable")?;
    private_directory(parent)?;
    let canonical =
        fs::canonicalize(parent).map_err(|_| "Remembered filter directory is unavailable")?;
    let _lock = PolicyLock::acquire(path)?;
    let previous = load_task_selection(path)?;
    let mut next = previous.clone();
    next.remember(suggested, selected);
    next.validate()?;
    if next == previous {
        return Ok(());
    }
    let bytes =
        serde_json::to_vec(&next).map_err(|_| "Remembered filter edits cannot be encoded")?;
    let temporary = canonical.join(format!(".task-filter-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Remembered filter staging failed")?;
        file.write_all(&bytes)
            .map_err(|_| "Remembered filter write failed")?;
        file.sync_all()
            .map_err(|_| "Remembered filter synchronization failed")?;
        verify_parent(parent, &canonical)?;
        if load_task_selection(path)? != previous {
            return Err("Remembered filter edits changed; review again.".into());
        }
        fs::rename(&temporary, path).map_err(|_| "Remembered filter publication failed")?;
        sync_directory(&canonical)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    if load_task_selection(path).is_ok_and(|stored| stored == next) {
        verify_parent(parent, &canonical)?;
        return sync_directory(&canonical);
    }
    result?;
    Err("Remembered filter readback did not match; no task was sent.".into())
}

fn save_inner(
    path: &Path,
    expected: &Policy,
    draft: &Policy,
    hook: &mut impl FnMut(SavePoint) -> Result<(), String>,
) -> Result<Policy, String> {
    expected.validate()?;
    draft.validate()?;
    if draft.revision != expected.revision {
        return Err("Privacy rules changed since this editor opened; reload before saving".into());
    }
    let mut next = draft.clone();
    next.revision = expected
        .revision
        .checked_add(1)
        .ok_or("Privacy rule revision exhausted")?;
    let bytes = serde_json::to_vec_pretty(&next).map_err(|_| "Privacy rules cannot be encoded")?;
    if bytes.len() > MAX_POLICY_BYTES {
        return Err("Privacy rules exceed the 1 MiB storage bound".into());
    }
    let parent = path
        .parent()
        .ok_or("Privacy rules have no project directory")?;
    check_owned_parents(path)?;
    private_directory(parent)?;
    let canonical_parent =
        fs::canonicalize(parent).map_err(|_| "Privacy project directory is unavailable")?;
    let name = path
        .file_name()
        .ok_or("Privacy rule filename is unavailable")?;
    let target = canonical_parent.join(name);
    let _lock = PolicyLock::acquire(&target)?;
    verify_parent(parent, &canonical_parent)?;
    let authoritative = load(&target)?;
    if authoritative != *expected {
        return Err("Privacy rules changed on disk; reload before saving this draft".into());
    }
    let result = write_atomic(&target, &bytes, expected, parent, &canonical_parent, hook);
    if let Err(error) = result {
        // An error after rename must not make the caller replay an already
        // committed change. Read back exactly this snapshot, then finish the
        // directory synchronization before reporting success.
        if load(&target).is_ok_and(|committed| committed == next) {
            verify_parent(parent, &canonical_parent)?;
            sync_directory(&canonical_parent)?;
            return Ok(next);
        }
        return Err(error);
    }
    if load(&target)? != next {
        return Err("Privacy rules changed during commit readback; reload them".into());
    }
    verify_parent(parent, &canonical_parent)?;
    Ok(next)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavePoint {
    BeforePublish,
    AfterPublish,
}

fn write_atomic(
    target: &Path,
    bytes: &[u8],
    expected: &Policy,
    original_parent: &Path,
    canonical_parent: &Path,
    hook: &mut impl FnMut(SavePoint) -> Result<(), String>,
) -> Result<(), String> {
    let temporary = canonical_parent.join(format!(".privacy-policy-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Privacy rule staging failed")?;
        file.write_all(bytes)
            .map_err(|_| "Privacy rule write failed")?;
        file.sync_all()
            .map_err(|_| "Privacy rule synchronization failed")?;
        drop(file);
        hook(SavePoint::BeforePublish)?;
        verify_parent(original_parent, canonical_parent)?;
        if load(target)? != *expected {
            return Err("Privacy rules changed while staging; reload before saving".into());
        }
        fs::rename(&temporary, target).map_err(|_| "Privacy rule publication failed")?;
        hook(SavePoint::AfterPublish)?;
        sync_directory(canonical_parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| "Privacy rule directory synchronization failed".into())
}

fn verify_parent(original: &Path, canonical: &Path) -> Result<(), String> {
    check_directory(original)?;
    if fs::canonicalize(original).map_err(|_| "Privacy project directory changed")? != canonical {
        return Err(
            "Privacy project directory changed during saving; reload before retrying".into(),
        );
    }
    Ok(())
}

// Guard every policy-owned directory through privacy/, while allowing trusted
// profile prefixes to contain platform aliases such as macOS's /var -> /private/var.
fn check_owned_parents(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Privacy rule directory is unavailable")?;
    let boundary = parent
        .ancestors()
        .find(|directory| directory.file_name().is_some_and(|name| name == "privacy"))
        .unwrap_or(parent);
    for directory in parent.ancestors() {
        match fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err("Privacy rule directory is not a real directory".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Privacy rule directory cannot be inspected".into()),
        }
        if directory == boundary {
            break;
        }
    }
    Ok(())
}

fn check_directory(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "Privacy project directory is unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Privacy project directory is not a real directory".into());
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => return check_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Privacy project directory cannot be inspected".into()),
    }
    let parent = path
        .parent()
        .ok_or("Privacy project directory has no parent")?;
    private_directory(parent)?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("Privacy project directory creation failed".into()),
    }
    check_directory(path)
}

struct PolicyLock {
    file: File,
}

impl PolicyLock {
    fn acquire(target: &Path) -> Result<Self, String> {
        let mut name = target
            .file_name()
            .ok_or("Privacy rule lock has no filename")?
            .to_os_string();
        name.push(".lock");
        let path = target.with_file_name(name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let file = options
                .open(&path)
                .map_err(|_| "Privacy rule lock is unavailable")?;
            let metadata = file
                .metadata()
                .map_err(|_| "Privacy rule lock cannot be inspected")?;
            if !metadata.is_file() {
                return Err("Privacy rule lock is not a regular file".into());
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| "Privacy rule lock permissions failed")?;
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return Err(
                    "Privacy rules are being saved elsewhere; retry after that save finishes"
                        .into(),
                );
            }
            use std::os::unix::fs::MetadataExt;
            let current = fs::symlink_metadata(&path).map_err(|_| "Privacy rule lock changed")?;
            if current.file_type().is_symlink()
                || !current.is_file()
                || current.dev() != metadata.dev()
                || current.ino() != metadata.ino()
            {
                return Err("Privacy rule lock identity changed; retry safely".into());
            }
            Ok(Self { file })
        }
        #[cfg(not(unix))]
        {
            let _ = (path, options);
            Err("Exclusive project privacy saving is unavailable on this platform".into())
        }
    }
}

impl Drop for PolicyLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::policy::{Action, Rule};
    use std::path::PathBuf;

    fn rule(action: Action) -> Rule {
        Rule {
            file: "private.json".into(),
            selector: "/document/*".into(),
            action,
        }
    }

    fn path(root: &tempfile::TempDir) -> PathBuf {
        root.path().join("privacy/projects/project/policy.json")
    }

    #[test]
    fn privacy_policy_stale_allow_cannot_replace_a_committed_hide() {
        let root = tempfile::tempdir().unwrap();
        let path = path(&root);
        let initial = load(&path).unwrap();
        let mut enabled = initial.clone();
        enabled.enabled = true;
        let snapshot = save(&path, &initial, &enabled).unwrap();
        let mut hide = snapshot.clone();
        hide.rules.push(rule(Action::Hide));
        let committed = save(&path, &snapshot, &hide).unwrap();
        let mut stale_allow = snapshot.clone();
        stale_allow.rules.push(rule(Action::Allow));
        assert!(save(&path, &snapshot, &stale_allow).is_err());
        assert_eq!(load(&path).unwrap(), committed);
    }

    #[test]
    fn privacy_policy_same_revision_manual_change_invalidates_expected_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = path(&root);
        let initial = load(&path).unwrap();
        let mut enabled = initial.clone();
        enabled.enabled = true;
        let expected = save(&path, &initial, &enabled).unwrap();
        let mut changed = expected.clone();
        changed.rules.push(rule(Action::Hide));
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        let mut draft = expected.clone();
        draft.rules.push(rule(Action::Allow));
        assert!(save(&path, &expected, &draft).is_err());
        assert_eq!(load(&path).unwrap(), changed);
    }

    #[test]
    fn privacy_policy_manual_change_during_staging_cannot_be_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let path = path(&root);
        let initial = load(&path).unwrap();
        let mut enabled = initial.clone();
        enabled.enabled = true;
        let expected = save(&path, &initial, &enabled).unwrap();
        let mut changed = expected.clone();
        changed.rules.push(rule(Action::Hide));
        let mut draft = expected.clone();
        draft.rules.push(rule(Action::Allow));
        assert!(save_inner(&path, &expected, &draft, &mut |point| {
            if point == SavePoint::BeforePublish {
                fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
            }
            Ok(())
        })
        .is_err());
        assert_eq!(load(&path).unwrap(), changed);
    }

    #[test]
    fn privacy_policy_save_failures_reconcile_only_exact_committed_snapshot() {
        for point in [SavePoint::BeforePublish, SavePoint::AfterPublish] {
            let root = tempfile::tempdir().unwrap();
            let path = path(&root);
            let expected = load(&path).unwrap();
            let mut draft = expected.clone();
            draft.enabled = true;
            let result = save_inner(&path, &expected, &draft, &mut |at| {
                if at == point {
                    Err("injected local save failure".into())
                } else {
                    Ok(())
                }
            });
            if point == SavePoint::BeforePublish {
                assert!(result.is_err());
                assert_eq!(load(&path).unwrap(), expected);
                assert_eq!(save(&path, &expected, &draft).unwrap().revision, 1);
            } else {
                let committed = result.unwrap();
                assert!(committed.enabled);
                assert_eq!(committed.revision, 1);
                assert_eq!(load(&path).unwrap(), committed);
            }
        }
    }

    #[test]
    fn privacy_policy_rejects_symlinks_invalid_files_and_bounds_without_resetting() {
        let root = tempfile::tempdir().unwrap();
        let path = path(&root);
        let expected = load(&path).unwrap();
        let mut draft = expected.clone();
        draft.enabled = true;
        save(&path, &expected, &draft).unwrap();
        fs::write(&path, b"not valid policy").unwrap();
        assert!(load(&path).is_err());
        fs::write(&path, vec![b'x'; MAX_POLICY_BYTES + 1]).unwrap();
        assert!(load(&path).is_err());
        #[cfg(unix)]
        {
            fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(root.path().join("missing"), &path).unwrap();
            assert!(load(&path).is_err());
            assert!(save(&path, &expected, &draft).is_err());
        }
    }

    #[test]
    fn privacy_policy_lock_is_stable_and_rejects_simultaneous_writers() {
        let root = tempfile::tempdir().unwrap();
        let path = path(&root);
        private_directory(path.parent().unwrap()).unwrap();
        let first = PolicyLock::acquire(&path).unwrap();
        assert!(PolicyLock::acquire(&path).is_err());
        let lock_path = path.with_file_name("policy.json.lock");
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&lock_path).unwrap().ino()
        };
        drop(first);
        let second = PolicyLock::acquire(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&lock_path).unwrap().ino(), inode);
        }
        drop(second);
        assert!(lock_path.exists());
    }

    #[test]
    fn privacy_context_refresh_detects_external_enable_and_same_revision_drift() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::privacy::Context::open(
            root.path().into(),
            root.path().into(),
            uuid::Uuid::new_v4(),
            None,
        )
        .unwrap();
        assert!(!context.enabled());
        let expected = context.policy();
        let mut enabled = expected.clone();
        enabled.enabled = true;
        let committed = save(&context.policy_path, &expected, &enabled).unwrap();
        assert!(!context.enabled());
        assert!(context.refresh_policy().unwrap());
        assert_eq!(context.policy(), committed);
        let private_revision = context.private_revision();
        context
            .project_source("private.json", "{\"sensitive\":123456}")
            .unwrap();
        assert!(!context.state.lock().unwrap().views.is_empty());
        let mut manual = committed.clone();
        manual.rules.push(rule(Action::Allow));
        fs::write(&context.policy_path, serde_json::to_vec(&manual).unwrap()).unwrap();
        assert!(context.check_policy(committed.revision).is_err());
        assert_eq!(context.policy(), manual);
        assert!(context.private_revision() > private_revision);
        assert!(context.state.lock().unwrap().views.is_empty());
        fs::remove_file(&context.policy_path).unwrap();
        assert!(context.refresh_policy().is_err());
        assert!(context.enabled());
    }

    #[test]
    fn privacy_context_external_newer_hide_rejects_stale_allow_draft() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::privacy::Context::open(
            root.path().into(),
            root.path().into(),
            uuid::Uuid::new_v4(),
            None,
        )
        .unwrap();
        let mut enabled = context.policy();
        enabled.enabled = true;
        context.save_policy(enabled).unwrap();
        let expected = context.policy();
        let mut hide = expected.clone();
        hide.rules.push(rule(Action::Hide));
        let committed = save(&context.policy_path, &expected, &hide).unwrap();
        let mut stale = expected.clone();
        stale.rules.push(rule(Action::Allow));
        assert!(context.save_policy(stale).is_err());
        assert_eq!(load(&context.policy_path).unwrap(), committed);
        assert!(context.refresh_policy().unwrap());
        assert_eq!(context.policy(), committed);
    }

    #[tokio::test]
    async fn privacy_disabling_policy_does_not_bypass_pending_edit_source_ticket() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::privacy::Context::open(
            root.path().into(),
            root.path().into(),
            uuid::Uuid::new_v4(),
            None,
        )
        .unwrap();
        context.allow_ephemeral();
        let mut policy = context.policy();
        policy.enabled = true;
        context.save_policy(policy).unwrap();
        let path = root.path().join("protected.txt");
        let original = "private original\n";
        fs::write(&path, original).unwrap();
        context.project_source("protected.txt", original).unwrap();
        let arguments = context
            .prepare_edit_ticket(
                "write_file",
                &serde_json::json!({
                    "path":path, "content":"new local content\n",
                }),
            )
            .unwrap();
        let mut disabled = context.policy();
        disabled.enabled = false;
        context.save_policy(disabled).unwrap();
        assert!(!context.enabled());
        let owner = crate::jobs::JobOwner::new(Some(context.session_id));
        assert!(crate::privacy::for_owner(owner).is_none());
        assert!(crate::privacy::for_edit_owner(owner).is_some());
        let concurrent = "concurrent local edit\n";
        fs::write(&path, concurrent).unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let result = crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
            "write_file",
            &arguments,
            &workspace,
            5,
            crate::sandbox::SandboxMode::PolicyOnly,
            false,
            owner,
        )
        .await;
        assert!(!result.is_success());
        assert_eq!(fs::read_to_string(&path).unwrap(), concurrent);
    }

    #[test]
    fn privacy_same_revision_drift_is_blocked_after_another_context_refreshes_first() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let scope = root.path().join("profile");
        let context = crate::privacy::Context::open(
            scope.clone(),
            workspace.clone(),
            uuid::Uuid::new_v4(),
            None,
        )
        .unwrap();
        context.allow_ephemeral();
        let mut allowed = context.policy();
        allowed.enabled = true;
        allowed.rules.push(Rule {
            file: "**".into(),
            selector: "**".into(),
            action: Action::Allow,
        });
        context.save_policy(allowed).unwrap();
        let private = "previously-allowed-private-canary";
        assert_eq!(
            context.project_source("private.txt", private).unwrap().text,
            private
        );
        let revision = context.policy().revision;
        let fingerprint = context.policy_fingerprint();
        let prepared = serde_json::to_vec(&serde_json::json!({
            "messages":[{"role":"user","content":private}]
        }))
        .unwrap();
        context
            .validate_wire(&prepared, revision, &fingerprint)
            .unwrap();
        let other =
            crate::privacy::Context::open(scope, workspace, uuid::Uuid::new_v4(), None).unwrap();
        let mut strict = context.policy();
        strict.rules.clear();
        fs::write(&context.policy_path, serde_json::to_vec(&strict).unwrap()).unwrap();
        assert!(other.refresh_policy().unwrap());
        assert_eq!(context.policy().revision, revision);
        assert_ne!(context.policy_fingerprint(), fingerprint);
        // This identity was formerly allowed, so there is no old private-value
        // alias for a string scan to detect. The bound full-policy receipt is
        // what must reject it, despite another context refreshing first.
        assert_eq!(context.mask_known(private).unwrap(), private);
        assert!(context
            .validate_wire(&prepared, revision, &fingerprint)
            .is_err());
    }

    #[test]
    fn privacy_new_hide_classifies_saved_receipts_even_without_source_history() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::privacy::Context::open(
            root.path().into(),
            root.path().into(),
            uuid::Uuid::new_v4(),
            None,
        )
        .unwrap();
        context.allow_ephemeral();
        let mut policy = context.policy();
        policy.enabled = true;
        policy.rules.push(Rule {
            file: "**".into(),
            selector: "**".into(),
            action: Action::Allow,
        });
        context.save_policy(policy).unwrap();
        let source = "{\"customer\":\"FormerAllowedPrivateCanary\",\"id\":428193}";
        let path = root.path().join("private.json");
        context
            .record_read(&path.to_string_lossy(), source, source, 1, 1, false)
            .unwrap();
        assert_eq!(
            context.mask_known("FormerAllowedPrivateCanary").unwrap(),
            "FormerAllowedPrivateCanary"
        );
        let mut hidden = context.policy();
        hidden.rules.clear();
        context.save_policy(hidden).unwrap();
        let fingerprint = context.policy_fingerprint();
        let revision = context.policy().revision;
        for body in [
            serde_json::json!({"messages":[{"role":"user","content":"FormerAllowedPrivateCanary"}]}),
            serde_json::json!({"messages":[{"role":"assistant","tool_calls":[{"function":{"name":"external_tool","arguments":"{\"id\":428193}"}}]}]}),
        ] {
            assert!(context
                .validate_wire(&serde_json::to_vec(&body).unwrap(), revision, &fingerprint)
                .is_err());
        }
        assert_ne!(
            context.mask_known("FormerAllowedPrivateCanary").unwrap(),
            "FormerAllowedPrivateCanary"
        );
        assert_ne!(context.mask_known("private.json").unwrap(), "private.json");
    }
}
