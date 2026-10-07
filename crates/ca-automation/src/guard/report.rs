//! Guard report construction.
//!
//! Target home for turning snapshots, globs, and diff facts into the structured
//! guard report used before verification/review.

use serde::{Deserialize, Serialize};

/// Local file-scope proof.  Computed from before/after snapshots, not from
/// model claims; first gate before verification or acceptance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct GuardReport {
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) changed_files: Vec<String>,
    pub(crate) allowed_patterns: Vec<String>,
    pub(crate) ignored_patterns: Vec<String>,
    pub(crate) violations: Vec<String>,
}

/// Cheap-enough snapshot fingerprint for a single file.
///
/// The hash catches same-length edits that mtime/len alone would miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileFingerprint {
    pub(crate) len: u64,
    pub(crate) modified_ns: u128,
    pub(crate) content_hash: u64,
}
