//! Host-only resources and frozen selections. No path is returned by a provider DTO.
//! Every mutation has a private, synchronized intent and a recoverable identity.
#[cfg(unix)]
mod native;
#[cfg(test)]
mod tests;

use crate::runtime::{atomic_write, private_dir, read, ResourceLease};
use anyhow::{bail, ensure, Context, Result};
use ca_automation_protocol::private_files::{
    FileCriteriaV1, FileEntryResultV1, FileMoveProgressV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const SCHEMA: &str = "ca.private-files.v1";
const MAX_MANIFEST: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 262_144;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredPathBindingV1 {
    pub spelling: String,
    pub reference: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredBindingsV1 {
    pub schema: String,
    pub message_key: String,
    pub authored_sha256: String,
    pub projected_sha256: String,
    pub paths: Vec<AuthoredPathBindingV1>,
    pub catalog: ca_automation_protocol::private_files::PrivateResourceCatalogV1,
    pub file_task: bool,
}

/// Unix path bytes, never lossy UTF-8. Only persisted in owner-only storage.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativePath(Vec<u8>);
impl NativePath {
    pub fn new(path: &Path) -> Self {
        Self(path.as_os_str().as_encoded_bytes().to_vec())
    }
    #[cfg(unix)]
    pub fn path(&self) -> PathBuf {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(self.0.clone()).into()
    }
    #[cfg(not(unix))]
    pub fn path(&self) -> PathBuf {
        String::from_utf8_lossy(&self.0).into_owned().into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fingerprint {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified: (i64, i64),
    pub changed: (i64, i64),
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub sha256: Option<String>,
}
impl Fingerprint {
    fn same_node(&self, other: &Self) -> bool {
        self.device == other.device && self.inode == other.inode
    }
    fn same_contents(&self, other: &Self) -> bool {
        self.same_node(other)
            && self.size == other.size
            && self.modified == other.modified
            && self.mode == other.mode
            && self.uid == other.uid
            && self.gid == other.gid
            && self.sha256 == other.sha256
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    schema: String,
    pub reference: String,
    pub path: NativePath,
    pub fingerprint: Fingerprint,
    pub folder: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub reference: String,
    pub relative: NativePath,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    schema: String,
    pub reference: String,
    pub version: u32,
    pub folder: String,
    pub destination: Option<String>,
    pub criteria: FileCriteriaV1,
    pub members: Vec<Member>,
    pub types: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Intent,
    Parked,
    Copying,
    Verified,
    Published,
    Moved,
    Collision,
    Changed,
    Unsupported,
}
impl Phase {
    fn terminal(&self) -> bool {
        matches!(
            self,
            Self::Moved | Self::Collision | Self::Changed | Self::Unsupported
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntryJournal {
    schema: String,
    file: String,
    phase: Phase,
    source: NativePath,
    destination: NativePath,
    source_fingerprint: Fingerprint,
    temporary: NativePath,
    parking: NativePath,
    parking_directory: Option<Fingerprint>,
    parked_fingerprint: Option<Fingerprint>,
    copy_fingerprint: Option<Fingerprint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoveBinding {
    pub schema: String,
    pub operation: String,
    pub selection: String,
    pub selection_sha256: String,
    pub destination: String,
    pub destination_fingerprint: Fingerprint,
    pub staging: NativePath,
}

#[derive(Clone, Debug)]
pub struct PrivateFiles {
    root: PathBuf,
}

type MoveFault<'a> = &'a dyn Fn(&str, usize) -> Result<()>;

#[derive(Default)]
pub struct MoveControl<'a> {
    pub cancel: Option<&'a dyn Fn() -> bool>,
    pub progress: Option<&'a dyn Fn(&FileMoveProgressV1)>,
    /// Fault injection is supplied by a caller, never by environment/provider data.
    pub fault: Option<MoveFault<'a>>,
}

fn reference(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}
fn supported_file_resource(fp: &Fingerprint) -> bool {
    fp.mode & libc::S_IFMT as u32 != libc::S_IFDIR as u32
}
fn public_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "pdf" => "pdf",
        "txt" => "txt",
        "md" => "md",
        "json" => "json",
        "csv" => "csv",
        "jpg" => "jpg",
        "jpeg" => "jpeg",
        "png" => "png",
        "zip" => "zip",
        _ => "other",
    }
}
fn check_ref(value: &str, prefix: &str) -> Result<()> {
    let suffix = value.strip_prefix(prefix).and_then(|s| s.strip_prefix('_'));
    ensure!(
        suffix.is_some_and(|s| s.len() == 32 && s.bytes().all(|c| c.is_ascii_hexdigit())),
        "invalid private resource reference"
    );
    Ok(())
}
fn sha(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

impl PrivateFiles {
    pub fn open(root: &Path) -> Result<Self> {
        private_dir(root)?;
        for name in [
            "resources",
            "selections",
            "operations",
            "locks",
            "bindings",
            "controls",
        ] {
            private_dir(&root.join(name))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// A host-issued anonymous DTO receipt. Arbitrary tool JSON must not gain
    /// a privacy exemption merely by matching its public shape.
    pub fn publish_control(&self, value: &serde_json::Value) -> Result<()> {
        let path = self
            .root
            .join("controls")
            .join(format!("{}.json", sha(value)?));
        if path.exists() {
            ensure!(
                read::<serde_json::Value>(&path, 4 * 1024 * 1024)? == *value,
                "public control receipt changed"
            );
            return Ok(());
        }
        atomic_write(&path, value)
    }
    pub fn is_public_control(&self, value: &serde_json::Value) -> bool {
        sha(value)
            .ok()
            .and_then(|digest| {
                read::<serde_json::Value>(
                    &self.root.join("controls").join(format!("{digest}.json")),
                    4 * 1024 * 1024,
                )
                .ok()
            })
            .is_some_and(|stored| stored == *value)
    }
    pub fn authored_bindings(&self, key: &str) -> Result<Option<AuthoredBindingsV1>> {
        ensure!(
            crate::runtime::valid_digest(key),
            "invalid authored binding key"
        );
        let path = self.root.join("bindings").join(format!("user-{key}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let bindings: AuthoredBindingsV1 = read(&path, 4 * 1024 * 1024)?;
        ensure!(
            bindings.schema == "ca.authored-private-paths.v1" && bindings.message_key == key,
            "authored private binding changed"
        );
        Ok(Some(bindings))
    }
    pub fn save_authored_bindings(&self, value: &AuthoredBindingsV1) -> Result<AuthoredBindingsV1> {
        let _lease = ResourceLease::acquire(&self.root.join("locks"), "broker")?;
        if let Some(existing) = self.authored_bindings(&value.message_key)? {
            ensure!(
                existing.authored_sha256 == value.authored_sha256,
                "authored binding content changed"
            );
            return Ok(existing);
        }
        atomic_write(
            &self
                .root
                .join("bindings")
                .join(format!("user-{}.json", value.message_key)),
            value,
        )?;
        Ok(value.clone())
    }
    pub fn bind_file(&self, path: &Path) -> Result<Resource> {
        ensure!(
            !self.is_managed_path(path),
            "host-owned metadata is not a private file resource"
        );
        let parent = path
            .parent()
            .context("file parent is missing")?
            .canonicalize()?;
        self.bind_folder(&parent)?;
        let _lease = ResourceLease::acquire(&self.root.join("locks"), "broker")?;
        let path = parent.join(path.file_name().context("file name is missing")?);
        let fp = native::fingerprint(&path, true)?;
        ensure!(
            supported_file_resource(&fp),
            "unsupported private file resource"
        );
        let reference = self
            .find_file(&path, &fp)?
            .unwrap_or_else(|| reference("file"));
        let resource = Resource {
            schema: SCHEMA.into(),
            reference,
            path: NativePath::new(&path),
            fingerprint: fp,
            folder: false,
        };
        self.save_resource(&resource)?;
        Ok(resource)
    }
    pub fn resolve_resource(&self, id: &str) -> Result<PathBuf> {
        let resource = self.resource(id)?;
        let path = resource.path.path();
        let actual = native::fingerprint(&path, !resource.folder)?;
        ensure!(
            if resource.folder {
                resource.fingerprint.same_node(&actual)
            } else {
                resource.fingerprint == actual
            },
            "private resource is stale or replaced"
        );
        Ok(path)
    }
    pub fn summarize(
        &self,
        id: &str,
        excluded: &[PathBuf],
    ) -> Result<ca_automation_protocol::private_files::PrivateResourceSummaryV1> {
        let resource = self.resource(id)?;
        let mut types = BTreeMap::<String, usize>::new();
        let mut count = 0;
        let root = resource.path.path();
        if resource.folder {
            for entry in walkdir::WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    !self.is_managed_path(e.path())
                        && !excluded.iter().any(|p| e.path().starts_with(p))
                })
            {
                let entry = entry?;
                if entry.file_type().is_dir() {
                    continue;
                }
                ensure!(count < MAX_FILES, "private inventory exceeds its limit");
                count += 1;
                *types.entry(public_type(entry.path()).into()).or_default() += 1;
            }
        } else {
            count = 1;
            types.insert(public_type(&root).into(), 1);
        }
        Ok(
            ca_automation_protocol::private_files::PrivateResourceSummaryV1 {
                reference: id.into(),
                kind: if resource.folder { "folder" } else { "file" }.into(),
                count,
                types,
            },
        )
    }
    pub fn is_managed_path(&self, path: &Path) -> bool {
        if path.starts_with(&self.root) {
            return true;
        }
        path.ancestors().any(|part| {
            part.file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix(".ca-private-"))
                .is_some_and(|id| {
                    check_ref(id, "move").is_ok()
                        && self
                            .root
                            .join("operations")
                            .join(id)
                            .join("binding.json")
                            .is_file()
                })
        })
    }

    /// Allocation is serialized by the ca broker, with a cross-process lease as
    /// defense against accidentally admitting a second owner after a restart.
    pub fn bind_folder(&self, path: &Path) -> Result<Resource> {
        let _lease = ResourceLease::acquire(&self.root.join("locks"), "broker")?;
        let path = path.canonicalize().context("folder is unavailable")?;
        ensure!(
            !self.is_managed_path(&path),
            "host-owned task metadata is not a file-task resource"
        );
        ensure!(path.is_dir(), "resource is not a folder");
        let fp = native::fingerprint(&path, false)?;
        for entry in std::fs::read_dir(self.root.join("resources"))? {
            let entry = entry?;
            if !entry.file_name().as_encoded_bytes().starts_with(b"folder_") {
                continue;
            }
            let resource: Resource = read(&entry.path(), 64 * 1024)?;
            if resource.folder && resource.path.path() == path {
                ensure!(
                    resource.fingerprint.same_node(&fp),
                    "bound folder was replaced"
                );
                return Ok(resource);
            }
        }
        let resource = Resource {
            schema: SCHEMA.into(),
            reference: reference("folder"),
            path: NativePath::new(&path),
            fingerprint: fp,
            folder: true,
        };
        self.save_resource(&resource)?;
        Ok(resource)
    }
    fn save_resource(&self, resource: &Resource) -> Result<()> {
        check_ref(
            &resource.reference,
            if resource.folder { "folder" } else { "file" },
        )?;
        atomic_write(
            &self
                .root
                .join("resources")
                .join(format!("{}.json", resource.reference)),
            resource,
        )?;
        if !resource.folder {
            let key = format!(
                "{:x}",
                Sha256::digest(resource.path.path().as_os_str().as_encoded_bytes())
            );
            atomic_write(
                &self
                    .root
                    .join("resources")
                    .join(format!("index-{key}.json")),
                &resource.reference,
            )?;
        }
        Ok(())
    }
    pub fn resource(&self, id: &str) -> Result<Resource> {
        check_ref(
            id,
            if id.starts_with("folder_") {
                "folder"
            } else {
                "file"
            },
        )?;
        let resource: Resource = read(
            &self.root.join("resources").join(format!("{id}.json")),
            64 * 1024,
        )?;
        ensure!(
            resource.schema == SCHEMA && resource.reference == id,
            "private resource binding changed"
        );
        Ok(resource)
    }
    pub fn folder(&self, id: &str) -> Result<Resource> {
        let r = self.resource(id)?;
        ensure!(r.folder, "folder reference required");
        ensure!(
            r.fingerprint
                .same_node(&native::fingerprint(&r.path.path(), false)?),
            "bound folder was replaced"
        );
        Ok(r)
    }
    pub fn folders(&self) -> Result<Vec<Resource>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.root.join("resources"))? {
            let entry = entry?;
            if !entry.file_name().as_encoded_bytes().starts_with(b"folder_") {
                continue;
            }
            let r: Resource = read(&entry.path(), 64 * 1024)?;
            if r.folder {
                out.push(r);
            }
        }
        Ok(out)
    }
    pub fn select(
        &self,
        folder: &str,
        destination: Option<&str>,
        criteria: FileCriteriaV1,
    ) -> Result<Selection> {
        self.select_excluding(folder, destination, criteria, &[])
    }
    pub fn select_excluding(
        &self,
        folder: &str,
        destination: Option<&str>,
        criteria: FileCriteriaV1,
        excluded: &[PathBuf],
    ) -> Result<Selection> {
        self.select_including(folder, destination, criteria, excluded, None)
    }
    /// Host-only projected predicates freeze native membership before moving.
    pub fn select_including(
        &self,
        folder: &str,
        destination: Option<&str>,
        criteria: FileCriteriaV1,
        excluded: &[PathBuf],
        included: Option<&BTreeSet<PathBuf>>,
    ) -> Result<Selection> {
        let resource = self.resource(folder)?;
        let explicit = (!resource.folder).then_some(resource.clone());
        let source = if resource.folder {
            self.folder(folder)?
        } else {
            let path = self.resolve_resource(folder)?;
            self.bind_folder(path.parent().context("private file has no parent")?)?
        };
        let _lease = ResourceLease::acquire(&self.root.join("locks"), "broker")?;
        ensure!(
            criteria.extensions.len() <= 64
                && criteria
                    .extensions
                    .iter()
                    .all(|s| s.len() <= 32 && !s.contains(['/', '\\', '\0'])),
            "invalid file criteria"
        );
        let root = source.path.path();
        let destination_path = destination
            .map(|id| self.folder(id).map(|r| r.path.path()))
            .transpose()?;
        ensure!(
            destination_path.as_ref() != Some(&root),
            "source and destination are the same folder"
        );
        let glob = criteria
            .name_glob
            .as_ref()
            .map(|s| {
                ensure!(s.len() <= 4096, "filename predicate is too long");
                Ok(globset::Glob::new(s)?.compile_matcher())
            })
            .transpose()?;
        let mut selection = Selection {
            schema: SCHEMA.into(),
            reference: reference("files"),
            version: 1,
            folder: source.reference,
            destination: destination.map(str::to_owned),
            criteria: criteria.clone(),
            members: Vec::new(),
            types: BTreeMap::new(),
        };
        let walk = walkdir::WalkDir::new(&root)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                !self.is_managed_path(entry.path())
                    && !excluded.iter().any(|p| entry.path().starts_with(p))
                    && destination_path
                        .as_ref()
                        .is_none_or(|dest| entry.path() != dest)
            });
        for entry in walk {
            let entry = entry.context("source enumeration failed")?;
            if entry.depth() == 0 || entry.file_type().is_dir() {
                continue;
            }
            let path = entry.path();
            if included.is_some_and(|included| !included.contains(path)) {
                continue;
            }
            if explicit
                .as_ref()
                .is_some_and(|file| file.path.path() != path)
            {
                continue;
            }
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !criteria.extensions.is_empty()
                && !criteria
                    .extensions
                    .iter()
                    .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&extension))
            {
                continue;
            }
            if glob
                .as_ref()
                .is_some_and(|g| !g.is_match(Path::new(entry.file_name())))
            {
                continue;
            }
            ensure!(
                selection.members.len() < MAX_FILES,
                "selection exceeds the local file limit"
            );
            let fp = native::fingerprint(path, true)?;
            if let Some(file) = &explicit {
                ensure!(
                    file.fingerprint == fp,
                    "explicit private file was changed; bind it again locally"
                );
            }
            let old = explicit
                .as_ref()
                .map(|f| Some(f.reference.clone()))
                .unwrap_or(self.find_file(path, &fp)?);
            let id = old.unwrap_or_else(|| reference("file"));
            self.save_resource(&Resource {
                schema: SCHEMA.into(),
                reference: id.clone(),
                path: NativePath::new(path),
                fingerprint: fp.clone(),
                folder: false,
            })?;
            selection.members.push(Member {
                reference: id,
                relative: NativePath::new(path.strip_prefix(&root)?),
                fingerprint: fp,
            });
            // Avoid disclosing arbitrary filename suffixes as supposedly public types.
            let type_name = match extension.as_str() {
                "pdf" | "txt" | "md" | "json" | "csv" | "jpg" | "jpeg" | "png" | "zip" => {
                    extension.as_str()
                }
                _ => "other",
            };
            *selection.types.entry(type_name.into()).or_default() += 1;
        }
        if let Some(file) = &explicit {
            ensure!(
                self.resolve_resource(&file.reference)? == file.path.path(),
                "explicit private file moved during selection"
            );
        }
        ensure!(
            serde_json::to_vec(&selection)?.len() as u64 <= MAX_MANIFEST,
            "private selection manifest exceeds its limit"
        );
        atomic_write(&self.selection_path(&selection.reference)?, &selection)?;
        Ok(selection)
    }
    fn find_file(&self, path: &Path, fp: &Fingerprint) -> Result<Option<String>> {
        // The per-folder index is only an accelerator; identities are revalidated.
        let key = format!("{:x}", Sha256::digest(path.as_os_str().as_encoded_bytes()));
        let index = self
            .root
            .join("resources")
            .join(format!("index-{key}.json"));
        if index.exists() {
            let id: String = read(&index, 256)?;
            let resource = self.resource(&id)?;
            if resource.path.path() == path && resource.fingerprint.same_contents(fp) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }
    fn selection_path(&self, id: &str) -> Result<PathBuf> {
        check_ref(id, "files")?;
        Ok(self.root.join("selections").join(format!("{id}.json")))
    }
    pub fn selection(&self, id: &str) -> Result<Selection> {
        let selection: Selection = read(&self.selection_path(id)?, MAX_MANIFEST)?;
        ensure!(
            selection.schema == SCHEMA
                && selection.reference == id
                && selection.version == 1
                && selection.members.len() <= MAX_FILES,
            "selection binding changed"
        );
        Ok(selection)
    }
    pub fn bind_move(&self, selection: &str, destination: &str) -> Result<MoveBinding> {
        let selected = self.selection(selection)?;
        let dest = self.folder(destination)?;
        let src = self.folder(&selected.folder)?;
        ensure!(
            src.path != dest.path,
            "source and destination are the same folder"
        );
        ensure!(
            selected
                .destination
                .as_deref()
                .is_none_or(|d| d == destination),
            "selection destination changed; select again"
        );
        if dest.path.path().starts_with(src.path.path()) {
            ensure!(
                selected.destination.as_deref() == Some(destination),
                "nested destinations must be excluded when selecting files"
            );
        }
        let selection_sha256 = sha(&selected)?;
        // The operation identity is deterministic for exactly this frozen intent.
        let operation = format!(
            "move_{}",
            &sha(&(selection, &selection_sha256, destination))?[..32]
        );
        let dir = self.root.join("operations").join(&operation);
        private_dir(&dir)?;
        let path = dir.join("binding.json");
        if path.exists() {
            let old: MoveBinding = read(&path, 64 * 1024)?;
            ensure!(
                old.schema == SCHEMA
                    && old.operation == operation
                    && old.selection == selection
                    && old.selection_sha256 == selection_sha256
                    && old.destination == destination
                    && old.destination_fingerprint.same_node(&dest.fingerprint),
                "operation binding changed"
            );
            return Ok(old);
        }
        let staging = dest.path.path().join(format!(".ca-private-{operation}"));
        let binding = MoveBinding {
            schema: SCHEMA.into(),
            operation,
            selection: selection.into(),
            selection_sha256,
            destination: destination.into(),
            destination_fingerprint: dest.fingerprint,
            staging: NativePath::new(&staging),
        };
        atomic_write(&path, &binding)?;
        Ok(binding)
    }

    pub fn execute(
        &self,
        binding: &MoveBinding,
        control: &MoveControl<'_>,
    ) -> Result<FileMoveProgressV1> {
        let _lease = ResourceLease::acquire(&self.root.join("locks"), &binding.operation)?;
        let selected = self.selection(&binding.selection)?;
        ensure!(
            sha(&selected)? == binding.selection_sha256,
            "frozen selection changed"
        );
        let destination = self.folder(&binding.destination)?;
        ensure!(
            destination
                .fingerprint
                .same_node(&binding.destination_fingerprint),
            "destination folder was replaced"
        );
        let source = self.folder(&selected.folder)?;
        let source_path = source.path.path();
        let dest_path = destination.path.path();
        let dir = self.root.join("operations").join(&binding.operation);
        let mut progress = FileMoveProgressV1 {
            operation: binding.operation.clone(),
            total: selected.members.len(),
            remaining: selected.members.len(),
            ..Default::default()
        };
        if let Some(report) = control.progress {
            report(&progress);
        }
        for (index, member) in selected.members.iter().enumerate() {
            let path = dir.join(format!("entry-{index:08}.json"));
            let relative = member.relative.path();
            ensure!(
                !relative.is_absolute()
                    && relative
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_))),
                "invalid selection member"
            );
            let from = source_path.join(&relative);
            let to = dest_path.join(&relative);
            let mut entry: EntryJournal = if path.exists() {
                let saved: EntryJournal = read(&path, 64 * 1024)?;
                ensure!(
                    saved.schema == SCHEMA
                        && saved.file == member.reference
                        && saved.source.path() == from
                        && saved.destination.path() == to
                        && saved.source_fingerprint == member.fingerprint,
                    "entry intent changed"
                );
                saved
            } else {
                if control.cancel.is_some_and(|f| f()) {
                    progress.cancelled = true;
                    break;
                }
                let e = EntryJournal {
                    schema: SCHEMA.into(),
                    file: member.reference.clone(),
                    phase: Phase::Intent,
                    source: NativePath::new(&from),
                    destination: NativePath::new(&to),
                    source_fingerprint: member.fingerprint.clone(),
                    temporary: NativePath::new(
                        &binding.staging.path().join(format!("entry-{index:08}.tmp")),
                    ),
                    parking: NativePath::new(
                        &from
                            .parent()
                            .context("source parent is missing")?
                            .join(format!(".ca-private-{}", binding.operation))
                            .join(format!("entry-{index:08}.source")),
                    ),
                    parking_directory: None,
                    parked_fingerprint: None,
                    copy_fingerprint: None,
                };
                Self::fault(control, "before_intent", index)?;
                atomic_write(&path, &e)?;
                Self::fault(control, "after_intent", index)?;
                e
            };
            if !entry.phase.terminal() {
                self.execute_entry(
                    &mut entry,
                    &path,
                    &source,
                    &destination,
                    binding,
                    control,
                    index,
                )?;
            }
            if entry.phase == Phase::Moved {
                // Location publication is idempotent even if the previous save failed.
                let mut resource = self.resource(&member.reference)?;
                resource.path = entry.destination.clone();
                let actual = native::fingerprint(&to, true)?;
                ensure!(
                    entry
                        .copy_fingerprint
                        .as_ref()
                        .unwrap_or(&entry.source_fingerprint)
                        .same_contents(&actual),
                    "completed destination was changed or replaced"
                );
                resource.fingerprint = actual;
                self.save_resource(&resource)?;
                progress.moved += 1;
            } else if entry.phase == Phase::Collision {
                progress.skipped += 1;
            } else {
                progress.errors += 1;
            }
            progress.remaining -= 1;
            if entry.phase != Phase::Moved && progress.entries.len() < 32 {
                progress.entries.push(FileEntryResultV1 {
                    file: member.reference.clone(),
                    status: format!("{:?}", entry.phase).to_ascii_lowercase(),
                });
            }
            Self::fault(control, "after_receipt", index)?;
            if let Some(report) = control.progress {
                report(&progress);
            }
            if control.cancel.is_some_and(|f| f()) {
                progress.cancelled = progress.remaining > 0;
                break;
            }
        }
        if progress.remaining == 0 {
            self.clean_staging(&dir, binding)?;
        }
        Self::fault(control, "before_batch_receipt", selected.members.len())?;
        atomic_write(&dir.join("progress.json"), &progress)?;
        Self::fault(control, "after_batch_receipt", selected.members.len())?;
        if let Some(report) = control.progress {
            report(&progress);
        }
        Ok(progress)
    }
    fn fault(control: &MoveControl<'_>, point: &str, index: usize) -> Result<()> {
        if let Some(f) = control.fault {
            f(point, index)?;
        }
        Ok(())
    }

    fn clean_staging(&self, journal: &Path, binding: &MoveBinding) -> Result<()> {
        let mut directories = BTreeMap::<PathBuf, Fingerprint>::new();
        for path in std::fs::read_dir(journal)? {
            let path = path?;
            if !path.file_name().as_encoded_bytes().starts_with(b"entry-") {
                continue;
            }
            let entry: EntryJournal = read(&path.path(), 64 * 1024)?;
            if let Some(identity) = entry.parking_directory {
                if let Some(parent) = entry.parking.path().parent() {
                    directories.insert(parent.to_path_buf(), identity);
                }
            }
        }
        if journal.join("staging.json").exists() {
            directories.insert(
                binding.staging.path(),
                read(&journal.join("staging.json"), 64 * 1024)?,
            );
        }
        for (path, identity) in directories {
            if !path.exists() {
                continue;
            }
            ensure!(
                identity.same_node(&native::fingerprint(&path, false)?),
                "private staging directory changed during cleanup"
            );
            if std::fs::read_dir(&path)?.next().is_none() {
                std::fs::remove_dir(&path)?;
                if let Some(parent) = path.parent() {
                    std::fs::File::open(parent)?.sync_all()?;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_entry(
        &self,
        e: &mut EntryJournal,
        journal: &Path,
        source: &Resource,
        destination: &Resource,
        binding: &MoveBinding,
        control: &MoveControl<'_>,
        index: usize,
    ) -> Result<()> {
        let from = native::Entry::open(
            &source.path.path(),
            &source.fingerprint,
            &e.source.path(),
            false,
        )?;
        let to = native::Entry::open(
            &destination.path.path(),
            &destination.fingerprint,
            &e.destination.path(),
            true,
        )?;
        let original = from.fingerprint(true)?;
        let dest = to.fingerprint(true)?;
        // Before parking there has been no filesystem effect. Even a hard link
        // to the original is a collision, not evidence of a completed move.
        if e.phase == Phase::Intent && e.parking_directory.is_none() {
            if dest.is_some() {
                e.phase = Phase::Collision;
                atomic_write(journal, e)?;
                return Ok(());
            }
            if original.as_ref() != Some(&e.source_fingerprint) {
                e.phase = Phase::Changed;
                atomic_write(journal, e)?;
                return Ok(());
            }
            if !native::supported(&e.source_fingerprint) {
                e.phase = Phase::Unsupported;
                atomic_write(journal, e)?;
                return Ok(());
            }
        }
        let parking_path = e.parking.path();
        let parking_parent = parking_path
            .parent()
            .context("source parking parent is missing")?;
        if let Some(expected) = &e.parking_directory {
            ensure!(
                expected.same_node(&native::fingerprint(parking_parent, false)?),
                "source parking directory changed"
            );
        } else {
            // No source can have been parked before this directory identity was
            // synchronized. A crash during its creation leaves only an empty
            // owner-only directory, which the identical intent may adopt.
            private_dir(parking_parent)?;
            e.parking_directory = Some(native::fingerprint(parking_parent, false)?);
            atomic_write(journal, e)?;
            Self::fault(control, "after_parking_intent", index)?;
        }
        let parked = native::Entry::open(
            &source.path.path(),
            &source.fingerprint,
            &parking_path,
            false,
        )?;
        let mut parked_fp = parked.fingerprint(true)?;
        if let Some(actual) = dest.as_ref() {
            let expected = e.copy_fingerprint.as_ref().unwrap_or(&e.source_fingerprint);
            if e.phase != Phase::Intent && expected.same_contents(actual) {
                e.phase = Phase::Published;
            } else if e.phase == Phase::Intent && parked_fp.is_none() {
                e.phase = Phase::Collision;
                atomic_write(journal, e)?;
                return Ok(());
            } else {
                bail!("destination identity needs local reconciliation");
            }
        } else if e.phase == Phase::Published {
            bail!("published destination disappeared; no action was replayed");
        }
        if e.phase == Phase::Published {
            if e.copy_fingerprint.is_some() {
                if let Some(actual) = parked_fp {
                    ensure!(
                        e.parked_fingerprint.as_ref() == Some(&actual),
                        "parked source changed after publication"
                    );
                    // Only the private parked name is removed. Any replacement
                    // at the user's original source name is never touched.
                    parked.unlink()?;
                    Self::fault(control, "after_source_remove", index)?;
                }
            } else {
                ensure!(
                    parked_fp.is_none(),
                    "published source still has a parked identity"
                );
            }
            e.phase = Phase::Moved;
            Self::fault(control, "before_entry_receipt", index)?;
            atomic_write(journal, e)?;
            return Ok(());
        }
        if e.phase == Phase::Intent {
            if parked_fp.is_none() {
                if original.as_ref() != Some(&e.source_fingerprint) {
                    e.phase = Phase::Changed;
                    atomic_write(journal, e)?;
                    return Ok(());
                }
                Self::fault(control, "before_park", index)?;
                from.rename_exclusive(&parked)
                    .context("source parking failed")?;
                Self::fault(control, "after_park", index)?;
                parked_fp = parked.fingerprint(true)?;
            }
            let actual = parked_fp.as_ref().context("parked source disappeared")?;
            if !e.source_fingerprint.same_contents(actual) {
                // A leaf replacement racing the parking rename is restored
                // without replacement. If its old name is occupied, both files
                // remain recoverable and the journal requires local attention.
                parked
                    .rename_exclusive(&from)
                    .context("changed source remains parked for local recovery")?;
                e.phase = Phase::Changed;
                atomic_write(journal, e)?;
                return Ok(());
            }
            e.parked_fingerprint = parked_fp.clone();
            e.phase = Phase::Parked;
            atomic_write(journal, e)?;
            Self::fault(control, "after_park_receipt", index)?;
        }
        ensure!(
            parked.fingerprint(true)?.as_ref() == e.parked_fingerprint.as_ref(),
            "parked source changed"
        );
        if e.phase == Phase::Parked {
            match parked.rename_exclusive(&to) {
                Ok(()) => {
                    Self::fault(control, "after_publish", index)?;
                    e.phase = Phase::Moved;
                    Self::fault(control, "before_entry_receipt", index)?;
                    atomic_write(journal, e)?;
                    return Ok(());
                }
                Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
                    parked
                        .rename_exclusive(&from)
                        .context("collision left original source parked for local recovery")?;
                    e.phase = Phase::Collision;
                    atomic_write(journal, e)?;
                    return Ok(());
                }
                Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {}
                Err(error) => return Err(error).context("exclusive move failed"),
            }
        }
        self.staging_identity(
            binding,
            journal.parent().context("private journal has no parent")?,
        )?;
        let temp = native::Entry::open(
            &destination.path.path(),
            &destination.fingerprint,
            &e.temporary.path(),
            false,
        )?;
        if matches!(e.phase, Phase::Parked | Phase::Copying) {
            if let Some(existing) = temp.fingerprint(false)? {
                ensure!(
                    (e.phase == Phase::Parked && e.copy_fingerprint.is_none())
                        || e.copy_fingerprint
                            .as_ref()
                            .is_some_and(|f| f.same_node(&existing)),
                    "temporary identity needs local reconciliation"
                );
                e.phase = Phase::Parked;
                e.copy_fingerprint = None;
                atomic_write(journal, e)?;
                temp.unlink()?;
            }
            e.phase = Phase::Copying;
            e.copy_fingerprint = Some(temp.create_from(&parked, false)?);
            Self::fault(control, "after_temp_create", index)?;
            atomic_write(journal, e)?;
            Self::fault(control, "after_temp_intent", index)?;
            temp.copy_from(&parked)?;
            let copied = temp
                .fingerprint(true)?
                .context("temporary copy disappeared")?;
            ensure!(
                copied.sha256 == e.source_fingerprint.sha256
                    && copied.size == e.source_fingerprint.size
                    && parked.fingerprint(true)?.as_ref() == e.parked_fingerprint.as_ref(),
                "copy verification failed or parked source changed"
            );
            e.copy_fingerprint = Some(copied);
            e.phase = Phase::Verified;
            atomic_write(journal, e)?;
            Self::fault(control, "after_verify", index)?;
        }
        ensure!(
            temp.fingerprint(true)?.as_ref().is_some_and(|actual| e
                .copy_fingerprint
                .as_ref()
                .is_some_and(|expected| expected.same_contents(actual))),
            "verified temporary copy changed"
        );
        match temp.rename_exclusive(&to) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
                parked
                    .rename_exclusive(&from)
                    .context("collision left original source parked for local recovery")?;
                temp.unlink()?;
                e.phase = Phase::Collision;
                atomic_write(journal, e)?;
                return Ok(());
            }
            Err(error) => return Err(error).context("copy publication failed"),
        }
        Self::fault(control, "after_publish", index)?;
        e.phase = Phase::Published;
        atomic_write(journal, e)?;
        Self::fault(control, "after_publish_receipt", index)?;
        ensure!(
            parked.fingerprint(true)?.as_ref() == e.parked_fingerprint.as_ref(),
            "parked source changed after copy publication"
        );
        Self::fault(control, "before_source_remove", index)?;
        parked.unlink()?;
        Self::fault(control, "after_source_remove", index)?;
        e.phase = Phase::Moved;
        Self::fault(control, "before_entry_receipt", index)?;
        atomic_write(journal, e)?;
        Ok(())
    }

    fn staging_identity(&self, binding: &MoveBinding, journal: &Path) -> Result<Fingerprint> {
        let path = journal.join("staging.json");
        if path.exists() {
            let expected: Fingerprint = read(&path, 64 * 1024)?;
            ensure!(
                expected.same_node(&native::fingerprint(&binding.staging.path(), false)?),
                "private staging folder changed"
            );
            return Ok(expected);
        }
        private_dir(&binding.staging.path())?;
        ensure!(
            std::fs::read_dir(binding.staging.path())?.next().is_none(),
            "unrecorded staging folder is not empty"
        );
        let identity = native::fingerprint(&binding.staging.path(), false)?;
        atomic_write(&path, &identity)?;
        Ok(identity)
    }
}
