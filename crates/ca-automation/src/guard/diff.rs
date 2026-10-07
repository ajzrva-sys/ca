//! Diff summaries and changed-file risk inputs.
//!
//! Target home for insertion/deletion counts, changed-file flags, and parsed
//! diff information used by review routing.

use std::collections::{BTreeSet, HashMap};

use super::report::FileFingerprint;

pub(crate) fn diff_snapshots(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
) -> Vec<String> {
    let mut keys = BTreeSet::new();
    keys.extend(before.keys().cloned());
    keys.extend(after.keys().cloned());

    keys.into_iter()
        .filter(|path| before.get(path) != after.get(path))
        .collect()
}

pub(crate) fn diff_control_plane(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
) -> Vec<String> {
    diff_snapshots(before, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(len: u64, modified_ns: u128, content_hash: u64) -> FileFingerprint {
        FileFingerprint {
            len,
            modified_ns,
            content_hash,
        }
    }

    fn snap(entries: &[(&str, FileFingerprint)]) -> HashMap<String, FileFingerprint> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn diff_snapshots_new_files_detected() {
        let before = snap(&[("src/existing.rs", fp(100, 1000, 111))]);
        let after = snap(&[
            ("src/existing.rs", fp(100, 1000, 111)),
            ("src/new_file.rs", fp(50, 2000, 222)),
        ]);
        let changed = diff_snapshots(&before, &after);
        assert_eq!(changed, vec![s("src/new_file.rs")]);
    }

    #[test]
    fn diff_snapshots_deleted_files_detected() {
        let before = snap(&[
            ("src/keep.rs", fp(100, 1000, 111)),
            ("src/remove.rs", fp(200, 1500, 333)),
        ]);
        let after = snap(&[("src/keep.rs", fp(100, 1000, 111))]);
        let changed = diff_snapshots(&before, &after);
        assert_eq!(changed, vec![s("src/remove.rs")]);
    }

    #[test]
    fn diff_snapshots_modified_files_detected_by_hash() {
        let before = snap(&[("src/main.rs", fp(100, 1000, 111))]);
        let after = snap(&[("src/main.rs", fp(100, 2000, 999))]);
        let changed = diff_snapshots(&before, &after);
        assert_eq!(changed, vec![s("src/main.rs")]);
    }

    #[test]
    fn diff_snapshots_modified_files_detected_by_length() {
        let before = snap(&[("src/main.rs", fp(100, 1000, 111))]);
        let after = snap(&[("src/main.rs", fp(200, 1000, 111))]);
        let changed = diff_snapshots(&before, &after);
        assert_eq!(changed, vec![s("src/main.rs")]);
    }

    #[test]
    fn diff_snapshots_unchanged_files_not_reported() {
        let before = snap(&[
            ("src/a.rs", fp(100, 1000, 111)),
            ("src/b.rs", fp(200, 2000, 222)),
        ]);
        let after = snap(&[
            ("src/a.rs", fp(100, 1000, 111)),
            ("src/b.rs", fp(200, 2000, 222)),
        ]);
        let changed = diff_snapshots(&before, &after);
        assert!(changed.is_empty());
    }

    #[test]
    fn diff_snapshots_empty_snapshots_return_empty() {
        let empty = HashMap::new();
        let changed = diff_snapshots(&empty, &empty);
        assert!(changed.is_empty());
    }

    #[test]
    fn diff_snapshots_mixed_changes_sorted_by_path() {
        let before = snap(&[
            ("src/b.rs", fp(100, 1000, 111)),
            ("src/c.rs", fp(200, 1000, 222)),
        ]);
        let after = snap(&[
            ("src/a.rs", fp(50, 3000, 333)),
            ("src/b.rs", fp(100, 1000, 111)), // unchanged
            ("src/c.rs", fp(200, 1000, 999)), // hash changed
        ]);
        let changed = diff_snapshots(&before, &after);
        // BTreeSet ordering → alphabetical
        assert_eq!(changed, vec![s("src/a.rs"), s("src/c.rs")]);
    }

    #[test]
    fn diff_control_plane_delegates_to_diff_snapshots() {
        let before = snap(&[("file.rs", fp(10, 100, 1))]);
        let after = snap(&[("file.rs", fp(10, 100, 2))]);
        let cp_result = diff_control_plane(&before, &after);
        let ds_result = diff_snapshots(&before, &after);
        assert_eq!(cp_result, ds_result);
    }

    // helper
    fn s(v: &str) -> String {
        v.to_string()
    }
}
