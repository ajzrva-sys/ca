//! Guard glob matching.
//!
//! Target home for allowed/read-only/ignored file pattern compilation and path
//! matching behavior.

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

pub(crate) fn compile_globs(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let normalized = normalize_glob_pattern(pattern);
        builder.add(
            Glob::new(&normalized).with_context(|| format!("invalid glob pattern {pattern:?}"))?,
        );
    }
    Ok(builder.build()?)
}

pub(crate) fn expand_allowed_file_patterns(patterns: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for pattern in patterns {
        let normalized = normalize_glob_pattern(pattern);
        if is_tracking_docs_alias(&normalized) {
            expanded.extend(
                tracking_doc_patterns()
                    .iter()
                    .map(|path| (*path).to_string()),
            );
        } else {
            expanded.push(normalized);
        }
    }
    expanded.sort();
    expanded.dedup();
    expanded
}

pub(crate) fn normalize_glob_pattern(pattern: &str) -> String {
    let mut pattern = pattern.replace('\\', "/");
    while let Some(stripped) = pattern.strip_prefix("./") {
        pattern = stripped.to_string();
    }
    while let Some(stripped) = pattern.strip_prefix('/') {
        pattern = stripped.to_string();
    }
    if pattern.is_empty() {
        "**".to_string()
    } else {
        pattern
    }
}

fn is_tracking_docs_alias(pattern: &str) -> bool {
    let normalized = pattern
        .trim_matches('`')
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_'], " ");
    matches!(
        normalized.as_str(),
        "tracking docs"
            | "tracking doc"
            | "control docs"
            | "control doc"
            | "workflow tracking docs"
            | "workflow tracking doc"
    )
}

fn tracking_doc_patterns() -> &'static [&'static str] {
    &[
        "NEXT_STEPS.md",
        "NEXT_STEPS_non-ai.md",
        "CONTINUITY.md",
        "TASK.md",
        "PLAN.md",
        "ROADMAP.md",
        "docs/ROADMAP.md",
        "Docs/ROADMAP.md",
        "Docs/.INDEX.md",
        "docs/.INDEX.md",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compile_globs ──────────────────────────────────────────────────

    #[test]
    fn compile_globs_wildcard_matches_source_files() {
        let set = compile_globs(&[s("src/**/*.rs")]).unwrap();
        assert!(set.is_match("src/main.rs"));
        assert!(set.is_match("src/deep/nested/module.rs"));
        assert!(!set.is_match("README.md"));
    }

    #[test]
    fn compile_globs_single_star_wildcard() {
        let set = compile_globs(&[s("*.toml")]).unwrap();
        assert!(set.is_match("Cargo.toml"));
        assert!(set.is_match("build.toml"));
        assert!(!set.is_match("Cargo.lock"));
    }

    #[test]
    fn compile_globs_exact_file_match() {
        let set = compile_globs(&[s("src/main.rs")]).unwrap();
        assert!(set.is_match("src/main.rs"));
        assert!(!set.is_match("src/lib.rs"));
    }

    #[test]
    fn compile_globs_nested_path() {
        let set = compile_globs(&[s("src/deep/nested/file.rs")]).unwrap();
        assert!(set.is_match("src/deep/nested/file.rs"));
        assert!(!set.is_match("src/deep/nested/other.rs"));
        assert!(!set.is_match("deep/nested/file.rs"));
    }

    #[test]
    fn compile_globs_multiple_patterns() {
        let set = compile_globs(&[s("src/**/*.rs"), s("Cargo.toml")]).unwrap();
        assert!(set.is_match("src/lib.rs"));
        assert!(set.is_match("Cargo.toml"));
        assert!(!set.is_match("README.md"));
    }

    #[test]
    fn compile_globs_invalid_pattern_returns_error() {
        let result = compile_globs(&[s("[invalid")]);
        assert!(result.is_err());
    }

    #[test]
    fn compile_globs_empty_list_succeeds_and_matches_nothing() {
        let set = compile_globs(&[]).unwrap();
        assert!(!set.is_match("anything.rs"));
        assert!(!set.is_match("src/main.rs"));
    }

    #[test]
    fn compile_globs_directory_wildcard() {
        let set = compile_globs(&[s("tests/**")]).unwrap();
        assert!(set.is_match("tests/integration.rs"));
        assert!(set.is_match("tests/deep/test.rs"));
        assert!(!set.is_match("src/tests.rs"));
    }

    // ── expand_allowed_file_patterns ───────────────────────────────────

    #[test]
    fn expand_normalizes_backslash_and_prefix() {
        let expanded = expand_allowed_file_patterns(&[s(".\\src\\main.rs")]);
        assert_eq!(expanded, vec![s("src/main.rs")]);
    }

    #[test]
    fn expand_tracking_docs_alias() {
        let expanded = expand_allowed_file_patterns(&[s("tracking docs")]);
        assert!(expanded.contains(&s("NEXT_STEPS.md")));
        assert!(expanded.contains(&s("CONTINUITY.md")));
        assert!(expanded.contains(&s("docs/ROADMAP.md")));
    }

    #[test]
    fn expand_tracking_docs_case_insensitive() {
        let expanded = expand_allowed_file_patterns(&[s("Tracking Docs")]);
        assert!(expanded.contains(&s("TASK.md")));
    }

    #[test]
    fn expand_deduplicates_patterns() {
        let expanded = expand_allowed_file_patterns(&[s("src/main.rs"), s("src/main.rs")]);
        assert_eq!(expanded.len(), 1);
    }

    // ── normalize_glob_pattern ─────────────────────────────────────────

    #[test]
    fn normalize_strips_leading_dot_slash() {
        assert_eq!(normalize_glob_pattern("./src/main.rs"), "src/main.rs");
    }

    #[test]
    fn normalize_strips_leading_slash() {
        assert_eq!(normalize_glob_pattern("/src/main.rs"), "src/main.rs");
    }

    #[test]
    fn normalize_replaces_backslash() {
        assert_eq!(normalize_glob_pattern("src\\lib.rs"), "src/lib.rs");
    }

    #[test]
    fn normalize_empty_becomes_double_star() {
        assert_eq!(normalize_glob_pattern(""), "**");
        assert_eq!(normalize_glob_pattern("/"), "**");
        assert_eq!(normalize_glob_pattern("./"), "**");
    }

    // helper
    fn s(v: &str) -> String {
        v.to_string()
    }
}
