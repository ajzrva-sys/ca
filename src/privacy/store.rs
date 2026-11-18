//! Local-only, immutable privacy aliases. Call filesystem methods on a worker.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const TOKEN_PREFIX: &str = "__CA_PRIVATE_";
pub const TOKEN_SUFFIX: &str = "__";
const REF_SCHEMA: &str = "ca.privacy_session.v1";
const MANIFEST_SCHEMA: &str = "ca.privacy_registry.v1";
const LEGACY_SEGMENT_SCHEMA: &str = "ca.privacy_alias_segment.v1";
const SEGMENT_SCHEMA: &str = "ca.privacy_alias_segment.v2";
const MAX_VALUE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 262_144;
const MAX_SEGMENTS: usize = 4096;
const SEGMENT_ENTRIES: usize = 4096;
const MAX_SEGMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
// Small originals share one immutable, synchronized segment. Large blocks keep
// content-addressed blobs so neither segment parsing nor peak allocation grows
// with an individual multi-megabyte source receipt.
const INLINE_VALUE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivateTaskContextRefV1 {
    pub schema: String,
    pub context_id: Uuid,
    pub marker_sha256: String,
}
impl PrivateTaskContextRefV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != "ca.private-task-context.v1"
            || self.context_id.is_nil()
            || !valid_digest(&self.marker_sha256)
        {
            return Err("private task context reference is invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivacySessionRef {
    pub schema: String,
    pub session_id: Uuid,
    pub registry_id: Uuid,
    pub manifest_sha256: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional_policy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_context: Option<PrivateTaskContextRefV1>,
    pub entries: usize,
    pub bytes: u64,
}

impl PrivacySessionRef {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(binding) = &self.task_context {
            binding.validate()?;
        }
        if self.schema != REF_SCHEMA
            || self.session_id.is_nil()
            || self.registry_id.is_nil()
            || !valid_digest(&self.manifest_sha256)
            || self.entries > MAX_ENTRIES
            || self.bytes > MAX_TOTAL_BYTES
        {
            return Err("privacy registry reference is invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AliasEntry {
    pub token: String,
    pub kind: String,
    pub value: String,
}

impl std::fmt::Debug for AliasEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliasEntry")
            .field("token", &self.token)
            .field("kind", &self.kind)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectRef {
    sha256: String,
    bytes: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEntry {
    token: String,
    kind: String,
    value: StoredValue,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum StoredValue {
    Blob(ObjectRef),
    Inline(InlineValue),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InlineValue {
    inline: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Segment {
    schema: String,
    registry_id: Uuid,
    entries: Vec<StoredEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    session_id: Uuid,
    registry_id: Uuid,
    namespace: String,
    next_counter: u64,
    entries: usize,
    bytes: u64,
    previous: Option<PrivacySessionRef>,
    total_segments: usize,
    segments: Vec<ObjectRef>,
}

/// Historical aliases are not the active privacy policy. Callers supply that
/// policy independently; never infer allow/deny decisions from these entries.
#[derive(Clone)]
pub struct AliasRegistry {
    session_id: Uuid,
    registry_id: Uuid,
    namespace: String,
    next_counter: u64,
    entries: Vec<AliasEntry>,
    by_value: BTreeMap<(String, String), usize>,
    /// First historical alias for an exact identity, independent of syntax.
    /// Local source receipts are deliberately kept outside this namespace.
    by_identity: BTreeMap<String, usize>,
    by_token: BTreeMap<String, usize>,
    reserved_tokens: HashSet<String>,
    bytes: u64,
    staged_entries: usize,
    segments: Vec<ObjectRef>,
    staged_ref: Option<PrivacySessionRef>,
}

impl std::fmt::Debug for AliasRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliasRegistry")
            .field("session_id", &self.session_id)
            .field("registry_id", &self.registry_id)
            .field("entries", &self.entries.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl AliasRegistry {
    pub fn new(session_id: Uuid) -> Self {
        let registry_id = Uuid::new_v4();
        Self {
            session_id,
            registry_id,
            namespace: registry_id.simple().to_string()[..8].to_string(),
            next_counter: 1,
            entries: Vec::new(),
            by_value: BTreeMap::new(),
            by_identity: BTreeMap::new(),
            by_token: BTreeMap::new(),
            reserved_tokens: HashSet::new(),
            bytes: 0,
            staged_entries: 0,
            segments: Vec::new(),
            staged_ref: None,
        }
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn entries(&self) -> &[AliasEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub fn lookup(&self, token: &str) -> Option<&str> {
        self.by_token
            .get(token)
            .map(|index| self.entries[*index].value.as_str())
    }

    pub fn alias(&mut self, kind: &str, value: &str) -> Result<String, String> {
        let kind = normalize_kind(kind)?;
        let identity = digest(value.as_bytes());
        let key = (kind.clone(), identity.clone());
        if !matches!(
            kind.as_str(),
            "SOURCE_RECEIPT" | "FILENAME_REGISTRY" | "NATIVE_DIGEST" | "TASK_FILTER"
        ) {
            if let Some(index) = self.by_identity.get(&identity) {
                if self.entries[*index].value != value {
                    return Err("privacy original identity collision".into());
                }
                return Ok(self.entries[*index].token.clone());
            }
        }
        if let Some(index) = self.by_value.get(&key) {
            if self.entries[*index].value != value {
                return Err("privacy original identity collision".into());
            }
            return Ok(self.entries[*index].token.clone());
        }
        if self.session_id.is_nil()
            || value.len() > MAX_VALUE_BYTES
            || self.entries.len() >= MAX_ENTRIES
            || self.bytes.saturating_add(value.len() as u64) > MAX_TOTAL_BYTES
        {
            return Err("privacy alias storage limit reached; no private data was sent".into());
        }
        collect_reserved_tokens(value, &mut self.reserved_tokens);
        let token = loop {
            let counter = self.next_counter;
            self.next_counter = counter
                .checked_add(1)
                .ok_or("privacy alias counter exhausted")?;
            let candidate = format!(
                "{TOKEN_PREFIX}{}_{kind}_{counter}{TOKEN_SUFFIX}",
                self.namespace
            );
            if !self.by_token.contains_key(&candidate) && !self.reserved_tokens.contains(&candidate)
            {
                break candidate;
            }
        };
        self.bytes += value.len() as u64;
        let index = self.entries.len();
        self.entries.push(AliasEntry {
            token: token.clone(),
            kind,
            value: value.to_string(),
        });
        self.by_value.insert(key, index);
        if !matches!(
            self.entries[index].kind.as_str(),
            "SOURCE_RECEIPT" | "FILENAME_REGISTRY" | "NATIVE_DIGEST" | "TASK_FILTER"
        ) {
            self.by_identity.insert(identity, index);
        }
        self.by_token.insert(token.clone(), index);
        Ok(token)
    }

    pub fn restore_exact(&self, token: &str) -> Result<&str, String> {
        self.by_token
            .get(token)
            .map(|index| &self.entries[*index])
            .filter(|entry| {
                !matches!(
                    entry.kind.as_str(),
                    "SOURCE_RECEIPT" | "FILENAME_REGISTRY" | "TASK_FILTER"
                )
            })
            .map(|entry| entry.value.as_str())
            .ok_or_else(|| "unknown privacy alias; local action was blocked".into())
    }

    /// Return validated alias ranges. An incomplete or unknown reserved token
    /// is rejected, including at EOF; values inserted by restoration are never
    /// scanned again.
    pub fn tokens_in(&self, text: &str) -> Result<Vec<Range<usize>>, String> {
        let mut ranges = Vec::new();
        let mut offset = 0;
        while let Some(start) = text[offset..].find(TOKEN_PREFIX) {
            let start = offset + start;
            let body = start + TOKEN_PREFIX.len();
            let Some(close) = text[body..].find(TOKEN_SUFFIX) else {
                return Err("incomplete privacy alias; local action was blocked".into());
            };
            let end = body + close + TOKEN_SUFFIX.len();
            let token = &text[start..end];
            if !valid_token(token) || self.restore_exact(token).is_err() {
                return Err("unknown privacy alias; local action was blocked".into());
            }
            ranges.push(start..end);
            offset = end;
        }
        Ok(ranges)
    }

    #[cfg(test)]
    pub fn restore_text(&self, text: &str) -> Result<String, String> {
        let ranges = self.tokens_in(text)?;
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for range in ranges {
            out.push_str(&text[copied..range.start]);
            out.push_str(self.restore_exact(&text[range.clone()])?);
            copied = range.end;
        }
        out.push_str(&text[copied..]);
        Ok(out)
    }

    /// Persist only the new alias delta, then an immutable manifest. This
    /// stages a reference; the session owner must commit it before sending.
    pub fn stage(&mut self, scope: &Path) -> Result<PrivacySessionRef, String> {
        self.stage_inner(scope, &mut |_| Ok(()))
    }

    fn stage_inner(
        &mut self,
        scope: &Path,
        hook: &mut impl FnMut(StagePoint) -> Result<(), String>,
    ) -> Result<PrivacySessionRef, String> {
        if self.staged_entries == self.entries.len() {
            if let Some(reference) = &self.staged_ref {
                return Ok(reference.clone());
            }
        }
        let dir = registry_directory(scope, self.session_id, true)?;
        let mut segments = Vec::new();
        let needed = self
            .entries
            .len()
            .saturating_sub(self.staged_entries)
            .div_ceil(SEGMENT_ENTRIES);
        if self.segments.len().saturating_add(needed) > MAX_SEGMENTS {
            return Err("privacy registry segment limit reached".into());
        }
        let mut entries = Vec::new();
        let mut entry_bytes = 0_usize;
        for entry in &self.entries[self.staged_entries..] {
            let value = if entry.value.len() <= INLINE_VALUE_BYTES {
                StoredValue::Inline(InlineValue {
                    inline: entry.value.clone(),
                })
            } else {
                StoredValue::Blob(write_object(
                    &dir.join("blobs"),
                    entry.value.as_bytes(),
                    MAX_VALUE_BYTES,
                )?)
            };
            let stored = StoredEntry {
                token: entry.token.clone(),
                kind: entry.kind.clone(),
                value,
            };
            let bytes = serde_json::to_vec(&stored)
                .map_err(|_| "privacy alias entry serialization failed")?
                .len();
            // The fixed segment header, UUID, closing delimiters and array
            // commas fit within this reserve. Exact bytes are bounded again
            // by write_object before any metadata references the segment.
            if !entries.is_empty()
                && (entries.len() == SEGMENT_ENTRIES
                    || entry_bytes
                        .saturating_add(bytes)
                        .saturating_add(entries.len())
                        .saturating_add(512)
                        > MAX_SEGMENT_BYTES)
            {
                if self.segments.len().saturating_add(segments.len()) >= MAX_SEGMENTS {
                    return Err("privacy registry segment limit reached".into());
                }
                segments.push(write_segment(
                    &dir,
                    self.registry_id,
                    std::mem::take(&mut entries),
                )?);
                entry_bytes = 0;
            }
            entries.push(stored);
            entry_bytes = entry_bytes.saturating_add(bytes);
        }
        if !entries.is_empty() {
            if self.segments.len().saturating_add(segments.len()) >= MAX_SEGMENTS {
                return Err("privacy registry segment limit reached".into());
            }
            segments.push(write_segment(&dir, self.registry_id, entries)?);
        }
        hook(StagePoint::BeforeManifest)?;
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            session_id: self.session_id,
            registry_id: self.registry_id,
            namespace: self.namespace.clone(),
            next_counter: self.next_counter,
            entries: self.entries.len(),
            bytes: self.bytes,
            previous: self.staged_ref.clone(),
            total_segments: self.segments.len() + segments.len(),
            segments,
        };
        let bytes =
            serde_json::to_vec(&manifest).map_err(|_| "privacy manifest serialization failed")?;
        let object = write_object(&dir.join("manifests"), &bytes, MAX_MANIFEST_BYTES)?;
        hook(StagePoint::AfterManifest)?;
        let reference = PrivacySessionRef {
            schema: REF_SCHEMA.into(),
            session_id: self.session_id,
            registry_id: self.registry_id,
            manifest_sha256: object.sha256,
            optional_policy: false,
            task_context: None,
            entries: self.entries.len(),
            bytes: self.bytes,
        };
        self.staged_entries = self.entries.len();
        self.segments.extend(manifest.segments);
        self.staged_ref = Some(reference.clone());
        Ok(reference)
    }

    pub fn validate_reference(
        scope: &Path,
        session_id: Uuid,
        reference: &PrivacySessionRef,
    ) -> Result<(), String> {
        read_manifest(scope, session_id, reference).map(|_| ())
    }

    pub fn load(
        scope: &Path,
        session_id: Uuid,
        reference: &PrivacySessionRef,
    ) -> Result<Self, String> {
        let manifest = read_manifest(scope, session_id, reference)?;
        let dir = registry_directory(scope, session_id, false)?;
        let mut registry = Self::new(session_id);
        registry.registry_id = manifest.registry_id;
        registry.namespace = manifest.namespace.clone();
        registry.next_counter = manifest.next_counter;
        let total_segments = manifest.total_segments;
        let mut chain = vec![manifest];
        while let Some(previous) = chain.last().and_then(|item| item.previous.as_ref()) {
            let current = chain.last().ok_or("privacy manifest chain is empty")?;
            if chain.len() > MAX_SEGMENTS
                || previous.session_id != session_id
                || previous.registry_id != registry.registry_id
                || previous.entries >= current.entries
                || previous.bytes > current.bytes
            {
                return Err("privacy manifest chain is invalid".into());
            }
            let earlier = read_manifest(scope, session_id, previous)?;
            if earlier.namespace != registry.namespace
                || earlier
                    .total_segments
                    .saturating_add(current.segments.len())
                    != current.total_segments
                || earlier.next_counter > current.next_counter
            {
                return Err("privacy manifest chain does not match".into());
            }
            chain.push(earlier);
        }
        let segments: Vec<ObjectRef> = chain
            .into_iter()
            .rev()
            .flat_map(|item| item.segments)
            .collect();
        if segments.len() != total_segments {
            return Err("privacy segment totals do not match".into());
        }
        let mut seen_segments = HashSet::new();
        for object in &segments {
            if !seen_segments.insert(object.sha256.clone()) {
                return Err("privacy registry contains a repeated segment".into());
            }
            let bytes = read_object(&dir.join("indexes"), object, MAX_SEGMENT_BYTES)?;
            let segment: Segment =
                serde_json::from_slice(&bytes).map_err(|_| "privacy alias segment is invalid")?;
            if !matches!(
                segment.schema.as_str(),
                SEGMENT_SCHEMA | LEGACY_SEGMENT_SCHEMA
            ) || segment.registry_id != registry.registry_id
                || segment.entries.is_empty()
                || segment.entries.len() > SEGMENT_ENTRIES
                || registry.entries.len().saturating_add(segment.entries.len()) > reference.entries
            {
                return Err("privacy alias segment identity is invalid".into());
            }
            for entry in segment.entries {
                let token_kind = entry
                    .token
                    .strip_prefix(TOKEN_PREFIX)
                    .and_then(|value| value.strip_suffix(TOKEN_SUFFIX))
                    .and_then(|value| value.split_once('_'))
                    .and_then(|(_, value)| value.rsplit_once('_'))
                    .map(|(kind, _)| kind);
                if normalize_kind(&entry.kind)? != entry.kind
                    || !valid_token(&entry.token)
                    || token_kind != Some(entry.kind.as_str())
                {
                    return Err("privacy alias entry is invalid".into());
                }
                let value = match entry.value {
                    StoredValue::Blob(object) => {
                        let bytes = read_object(&dir.join("blobs"), &object, MAX_VALUE_BYTES)?;
                        String::from_utf8(bytes)
                            .map_err(|_| "privacy original is not valid UTF-8")?
                    }
                    StoredValue::Inline(value) => {
                        if segment.schema != SEGMENT_SCHEMA
                            || value.inline.len() > INLINE_VALUE_BYTES
                        {
                            return Err("privacy inline original exceeds its format bounds".into());
                        }
                        value.inline
                    }
                };
                registry.bytes = registry.bytes.saturating_add(value.len() as u64);
                if registry.bytes > reference.bytes {
                    return Err("privacy registry exceeds its committed size".into());
                }
                let index = registry.entries.len();
                let identity = digest(value.as_bytes());
                if registry
                    .by_token
                    .insert(entry.token.clone(), index)
                    .is_some()
                {
                    return Err("privacy registry contains a duplicate alias".into());
                }
                let key = (entry.kind.clone(), identity.clone());
                if let Some(previous) = registry.by_value.get(&key) {
                    if registry.entries[*previous].value != value {
                        return Err("privacy original identity collision".into());
                    }
                } else {
                    registry.by_value.insert(key, index);
                }
                if !matches!(
                    entry.kind.as_str(),
                    "SOURCE_RECEIPT" | "FILENAME_REGISTRY" | "NATIVE_DIGEST" | "TASK_FILTER"
                ) {
                    if let Some(first) = registry.by_identity.get(&identity) {
                        if registry.entries[*first].value != value {
                            return Err("privacy original identity collision".into());
                        }
                    } else {
                        registry.by_identity.insert(identity, index);
                    }
                }
                collect_reserved_tokens(&value, &mut registry.reserved_tokens);
                registry.entries.push(AliasEntry {
                    token: entry.token,
                    kind: entry.kind,
                    value,
                });
            }
        }
        if registry.entries.len() != reference.entries || registry.bytes != reference.bytes {
            return Err("privacy registry does not match its committed totals".into());
        }
        registry.staged_entries = registry.entries.len();
        registry.segments = segments;
        registry.staged_ref = Some(reference.clone());
        Ok(registry)
    }

    /// Fork all issued mappings without touching profile/session storage. The
    /// first later stage must materialize inherited entries under the new owner.
    pub fn fork_in_memory(&self, session_id: Uuid) -> Result<Self, String> {
        if session_id == self.session_id || session_id.is_nil() {
            return Err("privacy branch identity is invalid".into());
        }
        let mut fork = Self::new(session_id);
        while fork.namespace == self.namespace
            || self.by_token.keys().any(|token| {
                token.strip_prefix(TOKEN_PREFIX).is_some_and(|rest| {
                    rest.strip_prefix(&fork.namespace)
                        .is_some_and(|tail| tail.starts_with('_'))
                })
            })
        {
            fork = Self::new(session_id);
        }
        fork.entries = self.entries.clone();
        fork.by_token = self.by_token.clone();
        fork.by_value = self.by_value.clone();
        fork.by_identity = self.by_identity.clone();
        fork.reserved_tokens = self.reserved_tokens.clone();
        fork.bytes = self.bytes;
        Ok(fork)
    }

    /// Detached review of a new task in the same session. Keep old tokens but
    /// allocate under a new namespace, so an older worker cannot collide.
    pub(crate) fn fork_for_task(&self) -> Result<Self, String> {
        let mut fork = self.fork_in_memory(Uuid::new_v4())?;
        fork.session_id = self.session_id;
        Ok(fork)
    }

    pub(crate) fn merge_task_history(&mut self, other: &Self) -> Result<(), String> {
        if self.session_id != other.session_id {
            return Err("Privacy history belongs to another session".into());
        }
        for entry in &other.entries {
            if matches!(entry.kind.as_str(), "TASK_FILTER" | "FILENAME_REGISTRY") {
                continue;
            }
            if let Some(index) = self.by_token.get(&entry.token) {
                let known = &self.entries[*index];
                if known.kind != entry.kind || known.value != entry.value {
                    return Err("Privacy history token collision".into());
                }
                continue;
            }
            if self.entries.len() >= MAX_ENTRIES
                || self.bytes.saturating_add(entry.value.len() as u64) > MAX_TOTAL_BYTES
            {
                return Err("Privacy history storage limit reached".into());
            }
            let index = self.entries.len();
            let identity = digest(entry.value.as_bytes());
            self.by_token.insert(entry.token.clone(), index);
            self.by_value
                .entry((entry.kind.clone(), identity.clone()))
                .or_insert(index);
            if !matches!(entry.kind.as_str(), "SOURCE_RECEIPT" | "NATIVE_DIGEST") {
                self.by_identity.entry(identity).or_insert(index);
            }
            collect_reserved_tokens(&entry.value, &mut self.reserved_tokens);
            self.bytes += entry.value.len() as u64;
            self.entries.push(entry.clone());
        }
        Ok(())
    }

    /// Materialize independent immutable sidecars for a branch. Inherited
    /// tokens remain exact; future allocations use the branch's new namespace.
    pub fn fork_to(&self, scope: &Path, session_id: Uuid) -> Result<Self, String> {
        let mut fork = self.fork_in_memory(session_id)?;
        fork.stage(scope)?;
        Ok(fork)
    }

    /// Active and archived sessions use the same profile scope. No sidecar
    /// move is required; validate the committed reference instead.
    pub fn archive(
        scope: &Path,
        session_id: Uuid,
        reference: &PrivacySessionRef,
    ) -> Result<(), String> {
        Self::validate_reference(scope, session_id, reference)
    }

    pub fn delete(scope: &Path, session_id: Uuid) -> Result<(), String> {
        let privacy = scope.join("privacy");
        if !privacy.exists() {
            return Ok(());
        }
        checked_directory(&privacy)?;
        let dir = privacy.join(session_id.to_string());
        if !dir.exists() {
            return Ok(());
        }
        checked_directory(&dir)?;
        fs::remove_dir_all(&dir).map_err(|_| "privacy sidecar cleanup failed")?;
        File::open(&privacy)
            .and_then(|file| file.sync_all())
            .map_err(|_| "privacy cleanup synchronization failed".into())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StagePoint {
    BeforeManifest,
    AfterManifest,
}

fn normalize_kind(kind: &str) -> Result<String, String> {
    if kind.is_empty()
        || kind.len() > 32
        || kind.starts_with('_')
        || kind.ends_with('_')
        || kind.contains("__")
        || !kind.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_')
    {
        return Err("privacy alias kind is invalid".into());
    }
    Ok(kind.to_ascii_uppercase())
}

fn valid_token(token: &str) -> bool {
    let Some(inner) = token
        .strip_prefix(TOKEN_PREFIX)
        .and_then(|v| v.strip_suffix(TOKEN_SUFFIX))
    else {
        return false;
    };
    let Some((namespace, remainder)) = inner.split_once('_') else {
        return false;
    };
    let Some((kind, counter)) = remainder.rsplit_once('_') else {
        return false;
    };
    namespace.len() == 8
        && namespace
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && normalize_kind(kind).is_ok_and(|normalized| normalized == kind)
        && !counter.is_empty()
        && counter.bytes().all(|b| b.is_ascii_digit())
        && counter.parse::<u64>().is_ok_and(|n| n > 0)
}

fn collect_reserved_tokens(text: &str, tokens: &mut HashSet<String>) {
    let mut rest = text;
    while let Some(start) = rest.find(TOKEN_PREFIX) {
        let candidate = &rest[start..];
        let Some(close) = candidate[TOKEN_PREFIX.len()..].find(TOKEN_SUFFIX) else {
            break;
        };
        let end = TOKEN_PREFIX.len() + close + TOKEN_SUFFIX.len();
        if valid_token(&candidate[..end]) {
            tokens.insert(candidate[..end].to_string());
        }
        rest = &candidate[end..];
    }
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_manifest(
    scope: &Path,
    session_id: Uuid,
    reference: &PrivacySessionRef,
) -> Result<Manifest, String> {
    reference.validate()?;
    if reference.session_id != session_id {
        return Err("privacy registry belongs to another session".into());
    }
    let dir = registry_directory(scope, session_id, false)?;
    let path = dir.join("manifests").join(&reference.manifest_sha256);
    let bytes = read_private(&path, MAX_MANIFEST_BYTES)?;
    if digest(&bytes) != reference.manifest_sha256 {
        return Err("privacy manifest digest does not match".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "privacy manifest is invalid")?;
    if manifest.schema != MANIFEST_SCHEMA
        || manifest.session_id != session_id
        || manifest.registry_id != reference.registry_id
        || manifest.entries != reference.entries
        || manifest.bytes != reference.bytes
        || manifest.total_segments > MAX_SEGMENTS
        || manifest.segments.len() > manifest.total_segments
        || (manifest.previous.is_none() && manifest.segments.len() != manifest.total_segments)
        || manifest.segments.len() > MAX_SEGMENTS
        || manifest.namespace.len() != 8
        || !manifest
            .namespace
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || manifest.next_counter == 0
        || manifest
            .segments
            .iter()
            .any(|o| !valid_digest(&o.sha256) || o.bytes > MAX_SEGMENT_BYTES)
    {
        return Err("privacy manifest identity or bounds are invalid".into());
    }
    Ok(manifest)
}

fn registry_directory(scope: &Path, session_id: Uuid, create: bool) -> Result<PathBuf, String> {
    if session_id.is_nil() {
        return Err("privacy registry session identity is invalid".into());
    }
    checked_directory(scope)?;
    let root = scope.join("privacy");
    let dir = root.join(session_id.to_string());
    for path in [&root, &dir] {
        if create {
            private_directory(path)?;
        } else {
            checked_directory(path)?;
        }
    }
    for name in ["blobs", "indexes", "manifests"] {
        let path = dir.join(name);
        if create {
            private_directory(&path)?;
        } else {
            checked_directory(&path)?;
        }
    }
    Ok(dir)
}

fn checked_directory(path: &Path) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "privacy storage directory is unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("privacy storage directory is invalid".into());
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("privacy storage directory creation failed".into()),
    }
    checked_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "privacy storage permissions failed")?;
    }
    Ok(())
}

fn read_private(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options
        .open(path)
        .map_err(|_| "privacy sidecar is unavailable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "privacy sidecar metadata failed")?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err("privacy sidecar exceeds its file bounds".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("privacy sidecar permissions are not private".into());
        }
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "privacy sidecar read failed")?;
    if bytes.len() != metadata.len() as usize || bytes.len() > maximum {
        return Err("privacy sidecar changed during its read".into());
    }
    Ok(bytes)
}

fn read_object(directory: &Path, object: &ObjectRef, maximum: usize) -> Result<Vec<u8>, String> {
    if !valid_digest(&object.sha256) || object.bytes > maximum {
        return Err("privacy object reference exceeds its bounds".into());
    }
    let bytes = read_private(&directory.join(&object.sha256), maximum)?;
    if bytes.len() != object.bytes || digest(&bytes) != object.sha256 {
        return Err("privacy object digest or size does not match".into());
    }
    Ok(bytes)
}

fn write_segment(
    directory: &Path,
    registry_id: Uuid,
    entries: Vec<StoredEntry>,
) -> Result<ObjectRef, String> {
    let segment = Segment {
        schema: SEGMENT_SCHEMA.into(),
        registry_id,
        entries,
    };
    let bytes =
        serde_json::to_vec(&segment).map_err(|_| "privacy alias segment serialization failed")?;
    write_object(&directory.join("indexes"), &bytes, MAX_SEGMENT_BYTES)
}

fn write_object(directory: &Path, bytes: &[u8], maximum: usize) -> Result<ObjectRef, String> {
    if bytes.len() > maximum {
        return Err("privacy object exceeds its storage bound".into());
    }
    let object = ObjectRef {
        sha256: digest(bytes),
        bytes: bytes.len(),
    };
    let path = directory.join(&object.sha256);
    if fs::symlink_metadata(&path).is_ok() {
        read_object(directory, &object, maximum)?;
        return Ok(object);
    }
    let temporary = directory.join(format!(".privacy-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let result: Result<ObjectRef, String> = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "privacy object creation failed")?;
        file.write_all(bytes)
            .map_err(|_| "privacy object write failed")?;
        file.sync_all()
            .map_err(|_| "privacy object synchronization failed")?;
        drop(file);
        fs::rename(&temporary, &path).map_err(|_| "privacy object publication failed")?;
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| "privacy object directory synchronization failed")?;
        Ok(object)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn opaque_aliases_round_trip_once_and_reject_unknown_tokens() {
        let mut registry = AliasRegistry::new(Uuid::new_v4());
        let inner = registry.alias("STRING", "private αβ").unwrap();
        let outer = registry
            .alias("STRING", &format!("literal {inner}"))
            .unwrap();
        assert_eq!(registry.alias("string", "private αβ").unwrap(), inner);
        assert_eq!(
            registry.restore_text(&outer).unwrap(),
            format!("literal {inner}")
        );
        assert_eq!(
            registry.restore_text(&format!("x {inner} y")).unwrap(),
            "x private αβ y"
        );
        assert!(registry
            .restore_text("__CA_PRIVATE_deadbeef_STRING_999__")
            .is_err());
        assert!(registry.restore_text("__CA_PRIVATE_incomplete").is_err());
        assert!(!format!("{registry:?}").contains("private αβ"));
        assert!(!format!("{:?}", registry.entries()).contains("private αβ"));
    }

    #[test]
    fn privacy_alias_identity_is_shared_across_html_css_js_and_scalar_kinds() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let original = "private_widget";
        let html = registry.alias("STRING", original).unwrap();
        let css = registry.alias("IDENTIFIER", original).unwrap();
        let javascript = registry.alias("STRING", original).unwrap();
        assert_eq!(html, css);
        assert_eq!(html, javascript);
        assert!(html.contains("_STRING_"));
        let receipt = registry.alias("SOURCE_RECEIPT", original).unwrap();
        assert_ne!(receipt, html);
        for (kind, value) in [("NUMBER", "123456"), ("BOOLEAN", "false"), ("NULL", "null")] {
            let typed = registry.alias(kind, value).unwrap();
            assert_eq!(registry.alias("STRING", value).unwrap(), typed);
            assert_eq!(registry.restore_exact(&typed).unwrap(), value);
        }
        let reference = registry.stage(scope.path()).unwrap();
        let mut loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
        assert_eq!(loaded.alias("IDENTIFIER", original).unwrap(), html);
        assert_eq!(loaded.lookup(&receipt), Some(original));
        assert!(loaded.restore_exact(&receipt).is_err());
        assert!(loaded.tokens_in(&receipt).is_err());
        assert!(
            super::super::shell::restore(&format!("printf '%s' '{receipt}'"), |token| loaded
                .restore_exact(token)
                .map(str::to_owned))
            .is_err()
        );
        let mut branch = loaded.fork_to(scope.path(), Uuid::new_v4()).unwrap();
        assert_eq!(branch.alias("KEY", original).unwrap(), html);
        assert_eq!(
            branch
                .restore_text(&format!("{html} {css} {javascript}"))
                .unwrap(),
            "private_widget private_widget private_widget"
        );
    }

    #[test]
    fn filename_registry_metadata_never_shares_or_restores_content_aliases() {
        fn check(
            registry: &mut AliasRegistry,
            kind: &str,
            value: &str,
            metadata: &str,
            content: &str,
        ) {
            assert_ne!(metadata, content);
            assert_eq!(registry.alias(kind, value).unwrap(), metadata);
            assert_eq!(registry.alias("IDENTIFIER", value).unwrap(), content);
            assert_eq!(registry.restore_exact(content).unwrap(), value);
            assert_eq!(registry.restore_text(content).unwrap(), value);
            assert_eq!(registry.tokens_in(content).unwrap().len(), 1);
            assert!(registry.restore_exact(metadata).is_err());
            assert!(registry.tokens_in(metadata).is_err());
            assert!(registry.restore_text(metadata).is_err());
            assert_eq!(registry.entries().len(), 2);
            assert!(registry.entries().iter().any(|entry| {
                entry.kind == kind && entry.token == metadata && entry.value == value
            }));
        }
        let value = r#"{"private_filename.rs":"fixture/f001.rs"}"#;
        for kind in ["SOURCE_RECEIPT", "FILENAME_REGISTRY", "TASK_FILTER"] {
            for metadata_first in [true, false] {
                let scope = TempDir::new().unwrap();
                let id = Uuid::new_v4();
                let mut registry = AliasRegistry::new(id);
                let (metadata, content) = if metadata_first {
                    let metadata = registry.alias(kind, value).unwrap();
                    (metadata, registry.alias("STRING", value).unwrap())
                } else {
                    let content = registry.alias("STRING", value).unwrap();
                    (registry.alias(kind, value).unwrap(), content)
                };
                check(&mut registry, kind, value, &metadata, &content);
                let reference = registry.stage(scope.path()).unwrap();
                let mut loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
                check(&mut loaded, kind, value, &metadata, &content);
                let fork_id = Uuid::new_v4();
                let mut fork = loaded.fork_to(scope.path(), fork_id).unwrap();
                check(&mut fork, kind, value, &metadata, &content);
                let reference = fork.stage(scope.path()).unwrap();
                let mut forked = AliasRegistry::load(scope.path(), fork_id, &reference).unwrap();
                check(&mut forked, kind, value, &metadata, &content);
            }
        }
    }

    #[test]
    fn privacy_historical_cross_kind_tokens_remain_restorable_after_loading() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let original = "historical_identity";
        let first = registry.alias("STRING", original).unwrap();
        // Reproduce a pre-deduplication registry without rewriting its history.
        let index = registry.entries.len();
        let second = format!(
            "{TOKEN_PREFIX}{}_IDENTIFIER_{}{TOKEN_SUFFIX}",
            registry.namespace, registry.next_counter
        );
        registry.next_counter += 1;
        registry.entries.push(AliasEntry {
            token: second.clone(),
            kind: "IDENTIFIER".into(),
            value: original.into(),
        });
        registry.by_token.insert(second.clone(), index);
        registry
            .by_value
            .insert(("IDENTIFIER".into(), digest(original.as_bytes())), index);
        registry.bytes += original.len() as u64;
        let reference = registry.stage(scope.path()).unwrap();
        let mut loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
        assert_eq!(loaded.entries().len(), 2);
        assert_eq!(loaded.restore_exact(&first).unwrap(), original);
        assert_eq!(loaded.restore_exact(&second).unwrap(), original);
        assert_eq!(loaded.alias("KEY", original).unwrap(), first);
    }

    #[test]
    fn aliases_skip_original_reserved_collisions_and_bound_large_blocks() {
        let mut registry = AliasRegistry::new(Uuid::new_v4());
        registry.namespace = "0123abcd".into();
        let token = registry
            .alias("STRING", "literal __CA_PRIVATE_0123abcd_STRING_1__")
            .unwrap();
        assert_eq!(token, "__CA_PRIVATE_0123abcd_STRING_2__");
        assert_eq!(
            registry.restore_text(&token).unwrap(),
            "literal __CA_PRIVATE_0123abcd_STRING_1__"
        );
        assert!(registry
            .alias("STRING", &"x".repeat(MAX_VALUE_BYTES + 1))
            .is_err());
        assert!(registry.alias("BAD__KIND", "value").is_err());
    }

    #[test]
    fn staged_deltas_load_large_values_and_do_not_rewrite_old_segments() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let value = "s".repeat(3 * 1024 * 1024);
        let token = registry.alias("BLOCK", &value).unwrap();
        let first = registry.stage(scope.path()).unwrap();
        let first_manifest = read_manifest(scope.path(), id, &first).unwrap();
        assert_eq!(first_manifest.segments.len(), 1);
        registry.alias("NUMBER", "123456789").unwrap();
        let second = registry.stage(scope.path()).unwrap();
        let manifest = read_manifest(scope.path(), id, &second).unwrap();
        assert_eq!(manifest.segments.len(), 1);
        assert_eq!(manifest.total_segments, 2);
        assert_eq!(manifest.previous.as_ref(), Some(&first));
        assert_eq!(
            first_manifest.segments[0].sha256,
            registry.segments[0].sha256
        );
        assert_eq!(
            AliasRegistry::load(scope.path(), id, &second)
                .unwrap()
                .restore_exact(&token)
                .unwrap(),
            value
        );
        assert_eq!(registry.stage(scope.path()).unwrap(), second);
        assert!(serde_json::to_vec(&second).unwrap().len() < 512);
    }

    #[test]
    fn failed_stage_retries_identical_aliases_before_and_after_manifest_publication() {
        for point in [StagePoint::BeforeManifest, StagePoint::AfterManifest] {
            let scope = TempDir::new().unwrap();
            let id = Uuid::new_v4();
            let mut registry = AliasRegistry::new(id);
            let before = registry.alias("STRING", "before").unwrap();
            let committed = registry.stage(scope.path()).unwrap();
            let after = registry.alias("STRING", "after").unwrap();
            assert!(registry
                .stage_inner(scope.path(), &mut |at| {
                    if at == point {
                        Err("injected local save failure".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err());
            let old = AliasRegistry::load(scope.path(), id, &committed).unwrap();
            assert_eq!(old.restore_exact(&before).unwrap(), "before");
            assert!(old.lookup(&after).is_none());
            let retried = registry.stage(scope.path()).unwrap();
            let loaded = AliasRegistry::load(scope.path(), id, &retried).unwrap();
            assert_eq!(loaded.restore_exact(&after).unwrap(), "after");
            assert_eq!(registry.alias("STRING", "after").unwrap(), after);
        }
    }

    #[test]
    fn corruption_missing_files_and_symlinks_fail_closed() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        // Large originals deliberately retain the content-addressed blob path.
        let original = "private source".repeat(INLINE_VALUE_BYTES / "private source".len() + 1);
        registry.alias("STRING", &original).unwrap();
        let reference = registry.stage(scope.path()).unwrap();
        let dir = registry_directory(scope.path(), id, false).unwrap();
        let value_digest = digest(original.as_bytes());
        fs::write(dir.join("blobs").join(&value_digest), b"changed").unwrap();
        assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        fs::remove_file(dir.join("blobs").join(&value_digest)).unwrap();
        assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                dir.join("manifests").join(&reference.manifest_sha256),
                dir.join("blobs").join(&value_digest),
            )
            .unwrap();
            assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        }
    }

    #[test]
    fn small_originals_share_bounded_immutable_segments_and_detect_corruption() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let mut first = String::new();
        for index in 0..10_000 {
            let token = registry
                .alias("IDENTIFIER", &format!("private_identifier_{index}"))
                .unwrap();
            if index == 0 {
                first = token;
            }
        }
        let reference = registry.stage(scope.path()).unwrap();
        let directory = registry_directory(scope.path(), id, false).unwrap();
        assert_eq!(fs::read_dir(directory.join("blobs")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(directory.join("indexes")).unwrap().count(), 3);
        let loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
        assert_eq!(loaded.entries().len(), 10_000);
        assert_eq!(
            loaded.restore_exact(&first).unwrap(),
            "private_identifier_0"
        );
        let manifest = read_manifest(scope.path(), id, &reference).unwrap();
        let object = directory.join("indexes").join(&manifest.segments[0].sha256);
        fs::write(&object, b"changed private inline value").unwrap();
        assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        fs::remove_file(&object).unwrap();
        assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                directory.join("manifests").join(&reference.manifest_sha256),
                &object,
            )
            .unwrap();
            assert!(AliasRegistry::load(scope.path(), id, &reference).is_err());
        }
    }

    #[test]
    fn inline_segments_split_by_encoded_size_and_round_trip_escaped_values() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let value = "\0".repeat(INLINE_VALUE_BYTES - 32);
        for index in 0..32 {
            registry
                .alias("STRING", &format!("{index}:{value}"))
                .unwrap();
        }
        let reference = registry.stage(scope.path()).unwrap();
        let manifest = read_manifest(scope.path(), id, &reference).unwrap();
        assert!(manifest.segments.len() > 1);
        assert!(manifest
            .segments
            .iter()
            .all(|segment| segment.bytes <= MAX_SEGMENT_BYTES));
        let loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
        assert_eq!(loaded.entries().len(), 32);
        assert_eq!(loaded.entries()[31].value, format!("31:{value}"));
    }

    #[test]
    fn legacy_v1_blob_segments_load_and_extend_without_rewriting_history() {
        let scope = TempDir::new().unwrap();
        let id = Uuid::new_v4();
        let mut registry = AliasRegistry::new(id);
        let token = registry.alias("STRING", "legacy private original").unwrap();
        let directory = registry_directory(scope.path(), id, true).unwrap();
        let value = write_object(
            &directory.join("blobs"),
            b"legacy private original",
            MAX_VALUE_BYTES,
        )
        .unwrap();
        let segment = Segment {
            schema: LEGACY_SEGMENT_SCHEMA.into(),
            registry_id: registry.registry_id,
            entries: vec![StoredEntry {
                token: token.clone(),
                kind: "STRING".into(),
                value: StoredValue::Blob(value),
            }],
        };
        let segment = write_object(
            &directory.join("indexes"),
            &serde_json::to_vec(&segment).unwrap(),
            MAX_SEGMENT_BYTES,
        )
        .unwrap();
        let old_segment = segment.sha256.clone();
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            session_id: id,
            registry_id: registry.registry_id,
            namespace: registry.namespace.clone(),
            next_counter: registry.next_counter,
            entries: 1,
            bytes: "legacy private original".len() as u64,
            previous: None,
            total_segments: 1,
            segments: vec![segment],
        };
        let object = write_object(
            &directory.join("manifests"),
            &serde_json::to_vec(&manifest).unwrap(),
            MAX_MANIFEST_BYTES,
        )
        .unwrap();
        let reference = PrivacySessionRef {
            schema: REF_SCHEMA.into(),
            session_id: id,
            registry_id: registry.registry_id,
            manifest_sha256: object.sha256,
            optional_policy: false,
            task_context: None,
            entries: 1,
            bytes: manifest.bytes,
        };
        let mut loaded = AliasRegistry::load(scope.path(), id, &reference).unwrap();
        assert_eq!(
            loaded.restore_exact(&token).unwrap(),
            "legacy private original"
        );
        let added = loaded
            .alias("STRING", "new inline private original")
            .unwrap();
        let advanced = loaded.stage(scope.path()).unwrap();
        let advanced_manifest = read_manifest(scope.path(), id, &advanced).unwrap();
        assert_eq!(advanced_manifest.previous.as_ref(), Some(&reference));
        assert!(directory.join("indexes").join(old_segment).is_file());
        let round_trip = AliasRegistry::load(scope.path(), id, &advanced).unwrap();
        assert_eq!(
            round_trip.restore_exact(&token).unwrap(),
            "legacy private original"
        );
        assert_eq!(
            round_trip.restore_exact(&added).unwrap(),
            "new inline private original"
        );
    }

    #[test]
    fn ephemeral_fork_retains_tokens_without_writes_and_first_stage_includes_every_entry() {
        let scope = TempDir::new().unwrap();
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        let mut parent = AliasRegistry::new(parent_id);
        let inherited = parent
            .alias("IDENTIFIER", "inherited private value")
            .unwrap();
        parent.stage(scope.path()).unwrap();
        // An entry not yet staged in the parent must be inherited as well.
        let unstaged = parent.alias("STRING", "unstaged private value").unwrap();
        let mut child = parent.fork_in_memory(child_id).unwrap();
        assert_ne!(child.registry_id, parent.registry_id);
        assert_ne!(child.namespace, parent.namespace);
        assert_eq!(child.next_counter, 1);
        assert_eq!(child.staged_entries, 0);
        assert!(child.staged_ref.is_none());
        assert!(child.segments.is_empty());
        assert_eq!(child.entries.len(), parent.entries.len());
        assert_eq!(
            child.restore_exact(&inherited).unwrap(),
            "inherited private value"
        );
        assert_eq!(
            child.restore_exact(&unstaged).unwrap(),
            "unstaged private value"
        );
        assert!(!scope
            .path()
            .join("privacy")
            .join(child_id.to_string())
            .exists());
        let parent_new = parent.alias("STRING", "new branch value").unwrap();
        let child_new = child.alias("STRING", "new branch value").unwrap();
        assert_ne!(parent_new, child_new);
        let reference = child.stage(scope.path()).unwrap();
        assert_eq!(reference.entries, child.entries.len());
        let loaded = AliasRegistry::load(scope.path(), child_id, &reference).unwrap();
        for (token, original) in [
            (&inherited, "inherited private value"),
            (&unstaged, "unstaged private value"),
            (&child_new, "new branch value"),
        ] {
            assert_eq!(loaded.restore_exact(token).unwrap(), original);
        }
        assert!(parent.fork_in_memory(parent_id).is_err());
        assert!(parent.fork_in_memory(Uuid::nil()).is_err());
    }

    #[test]
    fn branch_sidecars_survive_parent_cleanup_and_allocate_independently() {
        let scope = TempDir::new().unwrap();
        let parent_id = Uuid::new_v4();
        let child_id = Uuid::new_v4();
        let mut parent = AliasRegistry::new(parent_id);
        let inherited = parent.alias("IDENTIFIER", "private_name").unwrap();
        parent.stage(scope.path()).unwrap();
        let mut child = parent.fork_to(scope.path(), child_id).unwrap();
        let parent_new = parent.alias("STRING", "new value").unwrap();
        let child_new = child.alias("STRING", "new value").unwrap();
        assert_ne!(parent_new, child_new);
        let child_ref = child.stage(scope.path()).unwrap();
        AliasRegistry::archive(scope.path(), child_id, &child_ref).unwrap();
        AliasRegistry::delete(scope.path(), parent_id).unwrap();
        assert_eq!(
            AliasRegistry::load(scope.path(), child_id, &child_ref)
                .unwrap()
                .restore_exact(&inherited)
                .unwrap(),
            "private_name"
        );
    }
}
