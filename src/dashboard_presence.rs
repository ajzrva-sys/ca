//! Private, bounded, read-only cross-process observations. No control channel.
use crate::dashboard_snapshot::{bounded_text, DashboardEntry, DashboardIdentity, LiveStatus};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_INSTANCES: usize = 32;
const MAX_ROWS_PER_INSTANCE: usize = 16;
pub const STALE_AFTER_SECS: i64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceOwner {
    pub instance_id: Uuid,
    pub pid: u32,
    pub process_start: String,
    pub profile_storage_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceFocus {
    pub session_id: Option<Uuid>,
    pub terminal_focused: bool,
    pub ambiguous_modal: bool,
}

#[derive(Debug, Clone)]
pub struct PresenceFocusObservation {
    pub owner: PresenceOwner,
    pub focus: PresenceFocus,
}

impl PresenceOwner {
    pub fn current_for_scope(scope: &Path) -> Option<Self> {
        let pid = std::process::id();
        Some(Self {
            instance_id: crate::dashboard_snapshot::instance_id(),
            pid,
            process_start: process_start(pid)?,
            profile_storage_sha256: scope_digest(scope).ok()?,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresenceRecord {
    version: u8,
    owner: PresenceOwner,
    focus: PresenceFocus,
    observed_at: chrono::DateTime<chrono::Utc>,
    rows: Vec<PresenceRow>,
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresenceRow {
    identity: DashboardIdentity,
    status: LiveStatus,
    workspace: String,
    updated_at: chrono::DateTime<chrono::Utc>,
    counters: crate::dashboard_snapshot::ActivityCounters,
}

#[derive(Clone, Debug)]
pub struct PresenceStore {
    directory: PathBuf,
}

#[derive(Debug, Default)]
pub struct PresenceScan {
    pub entries: Vec<DashboardEntry>,
    pub unavailable_records: usize,
    pub truncated: bool,
    pub verified_focus: Vec<PresenceFocusObservation>,
    pub focus_unavailable: bool,
}

impl PresenceStore {
    /// All store methods perform filesystem work and belong on a blocking worker.
    pub fn open(directory: PathBuf) -> std::io::Result<Self> {
        if !directory.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = std::fs::DirBuilder::new();
                builder.mode(0o700);
                builder.create(&directory)?;
            }
            #[cfg(not(unix))]
            std::fs::create_dir(&directory)?;
        }
        validate_private(&directory, true)?;
        Ok(Self { directory })
    }

    pub fn publish(
        &self,
        owner: &PresenceOwner,
        entries: &[DashboardEntry],
        focus: PresenceFocus,
    ) -> std::io::Result<()> {
        use std::io::Write;
        validate_private(&self.directory, true)?;
        if owner.pid != std::process::id()
            || process_start(owner.pid).as_deref() != Some(owner.process_start.as_str())
        {
            return Err(std::io::Error::other(
                "presence publisher ownership changed",
            ));
        }
        let rows = entries
            .iter()
            .filter(|entry| !entry.external && entry.identity.instance_id == owner.instance_id)
            .take(MAX_ROWS_PER_INSTANCE)
            .map(|entry| PresenceRow {
                identity: entry.identity.clone(),
                status: entry.status,
                workspace: bounded_text(&entry.workspace, 1024),
                updated_at: entry.updated_at,
                counters: entry.counters.clone(),
            })
            .collect::<Vec<_>>();
        let record = PresenceRecord {
            version: 3,
            owner: owner.clone(),
            focus,
            observed_at: chrono::Utc::now(),
            truncated: entries.len() > rows.len(),
            rows,
        };
        let bytes = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(std::io::Error::other("presence metadata exceeds bound"));
        }
        let temporary =
            self.directory
                .join(format!(".{}.{}.tmp", owner.instance_id, Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let result = (|| {
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, self.path(owner.instance_id))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    #[cfg(test)]
    pub fn scan(
        &self,
        exclude: Uuid,
        now: chrono::DateTime<chrono::Utc>,
    ) -> std::io::Result<PresenceScan> {
        self.scan_with_sessions(exclude, now, None)
    }

    pub fn scan_with_sessions(
        &self,
        exclude: Uuid,
        now: chrono::DateTime<chrono::Utc>,
        sessions: Option<&crate::session::SessionStore>,
    ) -> std::io::Result<PresenceScan> {
        use std::io::Read;
        validate_private(&self.directory, true)?;
        let mut scan = PresenceScan::default();
        let mut files = Vec::new();
        let expected_scope =
            sessions.and_then(|store| scope_digest(store.access_coordinator().scope()).ok());
        let mut previews = std::collections::HashMap::<Uuid, String>::new();
        let preview_deadline = std::time::Instant::now() + std::time::Duration::from_millis(200);
        for (index, entry) in std::fs::read_dir(&self.directory)?.enumerate() {
            if index >= MAX_INSTANCES * 4 {
                scan.truncated = true;
                break;
            }
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                files.push(path);
            }
        }
        files.sort();
        if files.len() > MAX_INSTANCES {
            scan.truncated = true;
            files.truncate(MAX_INSTANCES);
        }
        for path in files {
            let Some(instance) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Uuid::parse_str(stem).ok())
            else {
                scan.unavailable_records += 1;
                continue;
            };
            if instance == exclude {
                continue;
            }
            let record = (|| -> std::io::Result<PresenceRecord> {
                validate_private(&path, false)?;
                let mut options = std::fs::OpenOptions::new();
                options.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
                }
                let file = options.open(&path)?;
                let metadata = file.metadata()?;
                if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
                    return Err(std::io::Error::other("invalid presence file"));
                }
                let mut bytes = Vec::new();
                file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_RECORD_BYTES {
                    return Err(std::io::Error::other("presence file grew past bound"));
                }
                serde_json::from_slice(&bytes).map_err(std::io::Error::other)
            })();
            let Ok(record) = record else {
                scan.unavailable_records += 1;
                continue;
            };
            let mut identities = std::collections::HashSet::new();
            if record.version != 3
                || record.owner.instance_id != instance
                || record.owner.pid == 0
                || record.owner.pid > i32::MAX as u32
                || record.owner.profile_storage_sha256.len() != 64
                || !record
                    .owner
                    .profile_storage_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || record.owner.process_start.len() > 128
                || record.rows.len() > MAX_ROWS_PER_INSTANCE
                || record.rows.iter().any(|row| {
                    row.identity.instance_id != instance || !identities.insert(row.identity.clone())
                })
            {
                scan.unavailable_records += 1;
                continue;
            }
            let status_override = if process_start(record.owner.pid).as_deref()
                != Some(record.owner.process_start.as_str())
            {
                Some(LiveStatus::Unavailable)
            } else if (now - record.observed_at).num_seconds() >= STALE_AFTER_SECS
                || record.observed_at > now + chrono::Duration::seconds(2)
            {
                Some(LiveStatus::Stale)
            } else {
                None
            };
            if status_override.is_none() {
                scan.verified_focus.push(PresenceFocusObservation {
                    owner: record.owner.clone(),
                    focus: record.focus.clone(),
                });
            } else if !process_provably_absent(record.owner.pid) {
                scan.focus_unavailable = true;
            }
            scan.truncated |= record.truncated;
            for row in record.rows {
                let status = status_override.unwrap_or(row.status);
                let kind = if row.identity.child_id.is_some() {
                    "Agent"
                } else {
                    "Session"
                };
                let mut preview = "External activity is read-only; controls stay in the owning process. Last committed preview unavailable.".to_string();
                if status_override.is_none()
                    && expected_scope.as_ref() == Some(&record.owner.profile_storage_sha256)
                {
                    if let (Some(session_id), Some(sessions)) = (row.identity.session_id, sessions)
                    {
                        if let Some(cached) = previews.get(&session_id) {
                            preview = cached.clone();
                        } else if previews.len() < 4 && std::time::Instant::now() < preview_deadline
                        {
                            if let Ok(saved) = sessions.load_recent_committed_bounded(session_id, 4)
                            {
                                if saved.session.id == session_id
                                    && process_start(record.owner.pid).as_deref()
                                        == Some(record.owner.process_start.as_str())
                                {
                                    let text = saved
                                        .session
                                        .messages
                                        .iter()
                                        .rev()
                                        .find(|message| {
                                            matches!(
                                                message.role,
                                                crate::app::Role::User
                                                    | crate::app::Role::Assistant
                                            ) && !message.content.trim().is_empty()
                                        })
                                        .map(|message| bounded_text(&message.content, 2048))
                                        .unwrap_or_else(|| "No committed message text".into());
                                    preview = format!("External activity is read-only.\nLast committed session transcript · {}\n\n{text}\n\nLive details and controls remain in the owning process.", saved.session.updated_at);
                                }
                            }
                            previews.insert(session_id, preview.clone());
                        }
                    }
                }
                scan.entries.push(DashboardEntry {
                    title: format!(
                        "{kind} · PID {} · pane {}",
                        record.owner.pid, row.identity.pane_id
                    ),
                    identity: row.identity,
                    status,
                    workspace: bounded_text(&row.workspace, 1024),
                    activity: format!("{} · external observation", status.label()),
                    preview,
                    updated_at: row.updated_at,
                    parent_turn: None,
                    external: true,
                    counters: row.counters,
                });
            }
        }
        scan.focus_unavailable |= scan.unavailable_records > 0 || scan.truncated;
        Ok(scan)
    }

    pub fn remove_owner(&self, owner: &PresenceOwner) -> std::io::Result<()> {
        validate_private(&self.directory, true)?;
        if owner.pid != std::process::id()
            || process_start(owner.pid).as_deref() != Some(owner.process_start.as_str())
        {
            return Err(std::io::Error::other("presence remover ownership changed"));
        }
        match std::fs::remove_file(self.path(owner.instance_id)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn path(&self, instance: Uuid) -> PathBuf {
        self.directory.join(format!("{instance}.json"))
    }
}

fn scope_digest(scope: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let canonical = scope.canonicalize()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.as_os_str().as_encoded_bytes())
    ))
}

pub fn profile_storage_digest(scope: &Path) -> std::io::Result<String> {
    scope_digest(scope)
}

fn process_provably_absent(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        pid > 0
            && unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn validate_private(path: &Path, directory: bool) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(std::io::Error::other("presence path has wrong type"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
            || (!directory && metadata.nlink() != 1)
        {
            return Err(std::io::Error::other(
                "presence path is not private to current user",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_bsdinfo>();
    let returned = unsafe {
        libc::proc_pidinfo(
            pid.try_into().ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if returned != size as i32 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.pbi_uid != unsafe { libc::geteuid() } {
        return None;
    }
    Some(format!(
        "{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    ))
}

#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let file = std::fs::File::open(format!("/proc/{pid}/stat")).ok()?;
    if file.metadata().ok()?.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let mut bytes = String::new();
    file.take(8192).read_to_string(&mut bytes).ok()?;
    let fields = bytes
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    Some(fields.get(19)?.to_string())
}

#[cfg(target_os = "freebsd")]
fn process_start(pid: u32) -> Option<String> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid.try_into().ok()?,
    ];
    let mut info = std::mem::MaybeUninit::<libc::kinfo_proc>::zeroed();
    let mut size = std::mem::size_of::<libc::kinfo_proc>();
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as _,
            info.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || size != std::mem::size_of::<libc::kinfo_proc>() {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.ki_structsize != i32::try_from(size).ok()?
        || info.ki_pid != pid as libc::pid_t
        || info.ki_uid != unsafe { libc::geteuid() }
    {
        return None;
    }
    Some(format!(
        "{}:{}",
        info.ki_start.tv_sec, info.ki_start.tv_usec
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
fn process_start(_pid: u32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_preview_requires_same_profile_fresh_owner_and_committed_text() {
        let temp = tempfile::tempdir().unwrap();
        let sessions = crate::session::SessionStore::new(temp.path().join("sessions"));
        let mut session = crate::session::SessionV4::new(
            "private title",
            temp.path().to_string_lossy(),
            "fixture",
            "fixture",
            vec![crate::app::Message {
                role: crate::app::Role::Assistant,
                content: format!("COMMITTED_REPLY {}", "é".repeat(4096)),
                tool_calls: None,
                thinking: None,
                response_items: Vec::new(),
                timestamp: chrono::Utc::now(),
            }],
        );
        sessions.save(&mut session).unwrap();
        let Some(owner) = PresenceOwner::current_for_scope(sessions.access_coordinator().scope())
        else {
            return;
        };
        let presence = PresenceStore::open(temp.path().join("presence")).unwrap();
        let entry = DashboardEntry {
            identity: DashboardIdentity {
                instance_id: owner.instance_id,
                pane_id: 1,
                session_id: Some(session.id),
                child_id: None,
            },
            status: LiveStatus::Working,
            title: "UNCOMMITTED_PRIVATE_TITLE".into(),
            workspace: "/tmp".into(),
            activity: "UNCOMMITTED_PRIVATE_COMMAND".into(),
            preview: "UNCOMMITTED_PRIVATE_LIVE_REPLY".into(),
            updated_at: chrono::Utc::now(),
            parent_turn: None,
            external: false,
            counters: Default::default(),
        };
        let focus = PresenceFocus {
            session_id: Some(session.id),
            terminal_focused: true,
            ambiguous_modal: false,
        };
        presence.publish(&owner, &[entry], focus.clone()).unwrap();
        let bytes = std::fs::read_to_string(presence.path(owner.instance_id)).unwrap();
        assert!(!bytes.contains("COMMITTED_REPLY"));
        assert!(!bytes.contains("PRIVATE"));
        let scan = presence
            .scan_with_sessions(Uuid::new_v4(), chrono::Utc::now(), Some(&sessions))
            .unwrap();
        assert_eq!(scan.verified_focus[0].focus, focus);
        assert_eq!(
            scan.verified_focus[0].owner.profile_storage_sha256,
            profile_storage_digest(sessions.access_coordinator().scope()).unwrap()
        );
        assert!(!scan.focus_unavailable);
        assert!(scan.entries[0]
            .preview
            .contains("Last committed session transcript"));
        assert!(scan.entries[0].preview.contains("COMMITTED_REPLY"));
        assert!(scan.entries[0].preview.len() < 2400);
        assert!(!scan.entries[0].preview.contains("UNCOMMITTED"));
        let different_sessions =
            crate::session::SessionStore::new(temp.path().join("different-profile"));
        std::fs::create_dir_all(different_sessions.access_coordinator().scope()).unwrap();
        let different = presence
            .scan_with_sessions(
                Uuid::new_v4(),
                chrono::Utc::now(),
                Some(&different_sessions),
            )
            .unwrap();
        assert!(!different.entries[0].preview.contains("COMMITTED_REPLY"));
        let stale = presence
            .scan_with_sessions(
                Uuid::new_v4(),
                chrono::Utc::now() + chrono::Duration::seconds(20),
                Some(&sessions),
            )
            .unwrap();
        assert!(!stale.entries[0].preview.contains("COMMITTED_REPLY"));
        assert!(stale.verified_focus.is_empty());
        assert!(stale.focus_unavailable);
        let mut record: PresenceRecord = serde_json::from_str(&bytes).unwrap();
        record.owner.process_start = "reused-pid-start".into();
        std::fs::write(
            presence.path(owner.instance_id),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let reused = presence
            .scan_with_sessions(Uuid::new_v4(), chrono::Utc::now(), Some(&sessions))
            .unwrap();
        assert!(reused.verified_focus.is_empty());
        assert!(reused.focus_unavailable);
        assert!(!reused.entries[0].preview.contains("COMMITTED_REPLY"));
        #[cfg(unix)]
        {
            let mut exited = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg("exit 0")
                .spawn()
                .unwrap();
            let dead_pid = exited.id();
            exited.wait().unwrap();
            if process_provably_absent(dead_pid) {
                record.owner.pid = dead_pid;
                std::fs::write(
                    presence.path(owner.instance_id),
                    serde_json::to_vec(&record).unwrap(),
                )
                .unwrap();
                let dead = presence
                    .scan_with_sessions(Uuid::new_v4(), chrono::Utc::now(), Some(&sessions))
                    .unwrap();
                assert_eq!(dead.entries[0].status, LiveStatus::Unavailable);
                assert!(dead.verified_focus.is_empty());
                assert!(
                    !dead.focus_unavailable,
                    "a proven dead owner must not mute notifications forever"
                );
            }
        }
    }

    #[test]
    fn private_presence_excludes_transcript_and_marks_stale_or_reused_owner() {
        let temp = tempfile::tempdir().unwrap();
        let store = PresenceStore::open(temp.path().join("presence")).unwrap();
        let Some(owner) = PresenceOwner::current_for_scope(temp.path()) else {
            return;
        };
        let entry = DashboardEntry {
            identity: DashboardIdentity {
                instance_id: owner.instance_id,
                pane_id: 1,
                session_id: Some(Uuid::new_v4()),
                child_id: None,
            },
            status: LiveStatus::NeedsInput,
            title: "PRIVATE PROMPT".into(),
            workspace: "/tmp".into(),
            activity: "PRIVATE COMMAND".into(),
            preview: "PRIVATE TRANSCRIPT".into(),
            updated_at: chrono::Utc::now(),
            parent_turn: None,
            external: false,
            counters: Default::default(),
        };
        store
            .publish(&owner, &[entry], PresenceFocus::default())
            .unwrap();
        let bytes = std::fs::read_to_string(store.path(owner.instance_id)).unwrap();
        assert!(!bytes.contains("PRIVATE"));
        let fresh = store.scan(Uuid::new_v4(), chrono::Utc::now()).unwrap();
        assert_eq!(fresh.entries[0].status, LiveStatus::NeedsInput);
        let stale = store
            .scan(
                Uuid::new_v4(),
                chrono::Utc::now() + chrono::Duration::seconds(20),
            )
            .unwrap();
        assert_eq!(stale.entries[0].status, LiveStatus::Stale);
        let mut record: PresenceRecord = serde_json::from_str(&bytes).unwrap();
        record.owner.process_start = "wrong-start".into();
        std::fs::write(
            store.path(owner.instance_id),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store
                .scan(Uuid::new_v4(), chrono::Utc::now())
                .unwrap()
                .entries[0]
                .status,
            LiveStatus::Unavailable
        );
        store.remove_owner(&owner).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn presence_rejects_symlink_and_unprivate_directory() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(PresenceStore::open(target.clone()).is_err());
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert!(PresenceStore::open(link).is_err());
    }
}
