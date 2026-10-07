//! Bounded, read-only discovery. Excerpts suggest an evaluation family; they
//! never prove a root cause or authorize code execution or promotion.
use crate::session_outcome::SessionOutcome;
use anyhow::{bail, Context, Result};
use codex_team::self_improvement::{
    canonical_digest, list_harness_cycles, load_harness_cycle, LearningVault,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FailureGroup {
    pub cohort: String,
    pub tool: String,
    pub provider: String,
    pub model: String,
    pub model_route: String,
    pub workspace_hash: String,
    pub evidence_ids: Vec<String>,
    pub independent_tasks: usize,
    pub latest_at: u64,
    pub family_hypothesis: Option<String>,
    /// Exact paths mentioned by the selected failures, still subject to the
    /// source freezer's preimage, clean-checkout and size checks.
    pub source_paths: Vec<String>,
}

#[derive(Serialize)]
pub struct Discovery {
    pub schema: &'static str,
    pub scanned_outcomes: usize,
    pub unreadable_outcomes: usize,
    pub groups: Vec<FailureGroup>,
}

fn family(excerpt: &str) -> Option<&'static str> {
    let text = excerpt.to_ascii_lowercase();
    let rules: &[(&str, &[&str])] = &[
        (
            "unicode_parser_boundary",
            &["char boundary", "utf-8 boundary", "truncate_utf8"],
        ),
        (
            "permission_scope_matching",
            &["is_path_allowed", "path escapes scope", "scope prefix"],
        ),
        (
            "protocol_tool_interleave",
            &["interleaved tool", "unknown tool call", "unfinished call"],
        ),
        (
            "atomic_journal_recovery",
            &[
                "truncated journal frame",
                "frame length mismatch",
                "journal recovery",
            ],
        ),
        (
            "process_timeout_cleanup",
            &[
                "descendant survived",
                "process group survived",
                "orphaned child",
            ],
        ),
        (
            "config_migration",
            &[
                "unsupported config version",
                "config migration",
                "legacy config",
            ],
        ),
    ];
    let mut matches = rules
        .iter()
        .filter(|(_, patterns)| patterns.iter().any(|pattern| text.contains(pattern)));
    let first = matches.next()?.0;
    matches.next().is_none().then_some(first)
}

/// Shared by preview and automatic admission. Only current-build, unconsumed,
/// independently attributable failures can enter a group. Deterministic order
/// makes a repeated preview select the same evidence.
pub(crate) fn select(
    outcomes: &[SessionOutcome],
    consumed: &BTreeSet<String>,
    build: &str,
) -> Result<Vec<FailureGroup>> {
    let mut groups: BTreeMap<
        String,
        Vec<(&SessionOutcome, &crate::session_outcome::EvidenceEvent)>,
    > = BTreeMap::new();
    let mut identities = BTreeMap::new();
    for outcome in outcomes {
        for event in &outcome.events {
            *identities.entry(event.id.as_str()).or_insert(0usize) += 1;
        }
    }
    for outcome in outcomes {
        if outcome.harness.build.as_deref() != Some(build) {
            continue;
        }
        for event in &outcome.events {
            if consumed.contains(&event.id)
                || identities.get(event.id.as_str()) != Some(&1)
                || crate::harness_freeze::validate_failure(outcome, event).is_err()
            {
                continue;
            }
            let cohort = canonical_digest(&(
                &outcome.workspace_hash,
                &outcome.harness,
                &outcome.provider,
                &outcome.model,
                &event.tool,
                family(&event.excerpt),
            ))?;
            groups.entry(cohort).or_default().push((outcome, event));
        }
    }
    let mut selected = Vec::new();
    for (cohort, mut events) in groups {
        events.sort_by(|a, b| {
            b.0.started_at
                .cmp(&a.0.started_at)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        let mut attempts = BTreeSet::new();
        events.retain(|(outcome, event)| {
            attempts.insert((&outcome.task_id, event.call_id.as_deref().unwrap()))
        });
        let roots: BTreeSet<_> = events
            .iter()
            .map(|(o, _)| o.independent_task_id())
            .collect();
        if events.len() < 3 || roots.len() < 2 {
            continue;
        }
        // Reserve one slot for a second independent root even when one task
        // contributes more than the maximum number of candidate events.
        let second = events
            .iter()
            .position(|(o, _)| o.independent_task_id() != events[0].0.independent_task_id())
            .unwrap();
        if second >= 64 {
            events.swap(63, second);
        }
        events.truncate(64);
        let ids = events.iter().map(|(_, e)| e.id.clone()).collect::<Vec<_>>();
        crate::harness_freeze::select_evidence(outcomes, &ids)?;
        let mut paths = BTreeSet::new();
        for (_, event) in &events {
            for token in event.excerpt.split(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '(' | ')' | '[' | ']' | ',')
            }) {
                let path = token.split(':').next().unwrap_or_default();
                if crate::harness_source_generation::editable_path(path) {
                    paths.insert(path.to_owned());
                }
            }
        }
        selected.push(FailureGroup {
            cohort,
            tool: events[0].1.tool.clone().unwrap(),
            provider: events[0].0.provider.clone().unwrap(),
            model: events[0].0.model.clone().unwrap(),
            model_route: events[0].0.harness.model_route.clone().unwrap(),
            workspace_hash: events[0].0.workspace_hash.clone(),
            evidence_ids: ids,
            independent_tasks: events
                .iter()
                .map(|(o, _)| o.independent_task_id())
                .collect::<BTreeSet<_>>()
                .len(),
            latest_at: events[0].0.started_at,
            family_hypothesis: family(&events[0].1.excerpt).map(str::to_owned),
            source_paths: paths.into_iter().take(8).collect(),
        });
    }
    selected.sort_by(|a, b| {
        b.latest_at
            .cmp(&a.latest_at)
            .then_with(|| a.cohort.cmp(&b.cohort))
    });
    Ok(selected)
}

pub(crate) fn discover(data_dir: &Path, vault: Option<&LearningVault>) -> Result<Discovery> {
    crate::harness_idle::preparation_boundary()?;
    let mut consumed = BTreeSet::new();
    if let Some(vault) = vault {
        for cycle in list_harness_cycles(vault)? {
            consumed.extend(load_harness_cycle(vault, &cycle.id)?.0.evidence_ids);
        }
    }
    let root = data_dir.join("outcomes");
    let mut outcomes = Vec::new();
    let mut unreadable = 0;
    if root.exists() {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&root)? {
            if entries.len() >= 4096 {
                bail!("outcome discovery exceeds 4096 entries");
            }
            entries.push(entry?);
        }
        entries.sort_by_key(|entry| entry.file_name());
        let mut remaining = 64u64 * 1024 * 1024;
        for entry in entries {
            crate::harness_idle::preparation_boundary()?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let result = (|| -> Result<SessionOutcome> {
                let bytes = crate::workspace_materialization::read_bounded_regular_file(
                    &root,
                    Path::new(&entry.file_name()),
                    remaining.min(2 * 1024 * 1024),
                )?;
                remaining = remaining
                    .checked_sub(bytes.len() as u64)
                    .context("outcome discovery exceeds byte bound")?;
                let outcome: SessionOutcome = serde_json::from_slice(&bytes)?;
                if path.file_stem().and_then(|s| s.to_str()) != Some(&outcome.task_id) {
                    bail!("outcome filename does not bind task identity");
                }
                Ok(outcome)
            })();
            match result {
                Ok(outcome) => outcomes.push(outcome),
                Err(_) => unreadable += 1,
            }
            if remaining == 0 {
                bail!("outcome discovery exceeds 64 MiB");
            }
        }
    }
    Ok(Discovery {
        schema: "ca.harness_discovery.v1",
        scanned_outcomes: outcomes.len(),
        unreadable_outcomes: unreadable,
        groups: select(&outcomes, &consumed, env!("CA_BUILD_DIGEST"))?,
    })
}

/// Choose only a group matching the configured route. A family signature is a
/// hypothesis to test; promotion still requires the full matched experiment.
pub(crate) fn select_for_provider<'a>(
    report: &'a Discovery,
    provider: &crate::harness_budget::FrozenHarnessProvider,
) -> Option<&'a FailureGroup> {
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Headless,
        &provider.source.provider,
        &provider.model,
        Vec::new(),
        Vec::new(),
        provider.options.clone(),
    );
    let identity = crate::harness_registry::capture_prepared_on_route(&request, &provider.source)
        .outcome_snapshot();
    report.groups.iter().find(|group| {
        group.family_hypothesis.is_some()
            && group.provider == provider.source.provider
            && group.model == provider.model
            && Some(&group.model_route) == identity.model_route.as_ref()
    })
}

/// Automatic source edits require diagnostic paths from this exact checkout.
/// Other projects and dirty checkouts can still propose runtime guidance.
pub(crate) fn selected_source(repository: &Path, group: &FailureGroup) -> Vec<String> {
    use sha2::{Digest, Sha256};
    let Ok(root) = repository.canonicalize() else {
        return Vec::new();
    };
    let Some(name) = root.to_str() else {
        return Vec::new();
    };
    if format!("{:x}", Sha256::digest(name.as_bytes())) != group.workspace_hash
        || group.source_paths.is_empty()
        || codex_team::self_improvement::observe_clean_repository(&root).is_err()
        || crate::harness_source_generation::read_source_files(&root, &group.source_paths).is_err()
    {
        return Vec::new();
    }
    group.source_paths.clone()
}

/// Runs on the creation worker. Reuses the explicit freezer so discovered IDs,
/// source preimages, route and all nine cases receive identical validation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_automatic(
    vault: &LearningVault,
    repository: &Path,
    binary: &Path,
    data_dir: &Path,
    provider: crate::harness_budget::FrozenHarnessProvider,
    instructions: String,
    enforce_cooldown: bool,
    vm_profile: Option<&codex_team::self_improvement::HarnessVmProfile>,
) -> Result<codex_team::self_improvement::HarnessPlan> {
    let preparation_started = std::time::Instant::now();
    let report = discover(data_dir, Some(vault))?;
    let group = select_for_provider(&report, &provider)
        .context("no unused supported failure group matches the configured build and route")?;
    let family = group.family_hypothesis.as_deref().unwrap();
    let cases = crate::harness_catalog::automatic_cases(repository, family)?;
    let temporary = tempfile::Builder::new()
        .prefix("discovered-cases-")
        .tempdir_in(vault.resolve_relative("harness/inputs")?)?;
    let target = temporary.path().join("target");
    let held_a = temporary.path().join("held-a");
    let held_b = temporary.path().join("held-b");
    for (case, path) in cases.iter().skip(6).zip([&target, &held_a, &held_b]) {
        crate::harness_idle::preparation_boundary()?;
        case.materialize(path)?;
    }
    let source_paths = selected_source(repository, group);
    crate::harness_freeze::create_cycle_with_preparation(
        vault,
        repository,
        binary,
        data_dir,
        &group.evidence_ids,
        provider,
        instructions,
        family,
        &target,
        [&held_a, &held_b],
        &source_paths,
        preparation_started,
        enforce_cooldown,
        vm_profile,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn records() -> Vec<SessionOutcome> {
        let mut records = crate::harness_freeze::tests::outcomes();
        for record in &mut records {
            for event in &mut record.events {
                if event.kind == "tool_evidence" {
                    event.excerpt =
                        "byte index is not a char boundary at src/parser.rs:42:3".into();
                }
            }
        }
        records
    }
    #[test]
    fn hrs253_discovery_groups_only_fresh_independent_matching_evidence() {
        let mut records = records();
        let build = records[0].harness.build.clone().unwrap();
        let groups = select(&records, &BTreeSet::new(), &build).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].independent_tasks, 2);
        assert_eq!(
            groups[0].family_hypothesis.as_deref(),
            Some("unicode_parser_boundary")
        );
        assert_eq!(groups[0].source_paths, ["src/parser.rs"]);
        assert!(select(
            &records,
            &groups[0].evidence_ids.iter().cloned().collect(),
            &build
        )
        .unwrap()
        .is_empty());
        assert!(select(&records, &BTreeSet::new(), "another-build")
            .unwrap()
            .is_empty());
        records[1].root_task_id = Some(records[0].task_id.clone());
        assert!(select(&records, &BTreeSet::new(), &build)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn hrs253_discovery_rejects_ambiguous_denied_incomplete_and_mixed_records() {
        let records = records();
        let build = records[0].harness.build.clone().unwrap();
        for variation in 0..6 {
            let mut changed = records.clone();
            match variation {
                0 => changed[1].evidence_complete = false,
                1 => changed[1].workspace_hash = "another-workspace".into(),
                2 => changed[1].events[0].excerpt = "Tool denied by user".into(),
                3 => changed.push(changed[1].clone()),
                4 => changed[1].harness.model_route = Some("another-route".into()),
                _ => {
                    let mut duplicate = changed[1].events[1].clone();
                    duplicate.id = format!("{}:99", changed[1].task_id);
                    duplicate.sequence = 99;
                    changed[1].events.push(duplicate);
                    changed[1].events[3].succeeded = Some(true);
                }
            }
            // The denied record still has one independent valid attempt; that
            // remains eligible with the first root's two failures.
            if variation == 2 {
                assert_eq!(
                    select(&changed, &BTreeSet::new(), &build).unwrap()[0]
                        .evidence_ids
                        .len(),
                    3
                );
            } else {
                assert!(select(&changed, &BTreeSet::new(), &build)
                    .unwrap()
                    .is_empty());
            }
        }
        assert!(family("char boundary and orphaned child").is_none());
        assert!(family("generic command failure").is_none());
    }
    #[test]
    fn hrs253_automatic_selection_matches_route_and_uses_source_only_from_clean_origin() {
        use sha2::{Digest, Sha256};
        let records = records();
        let groups = select(
            &records,
            &BTreeSet::new(),
            records[0].harness.build.as_deref().unwrap(),
        )
        .unwrap();
        let mut report = Discovery {
            schema: "ca.harness_discovery.v1",
            scanned_outcomes: 2,
            unreadable_outcomes: 0,
            groups,
        };
        let mut provider = crate::harness_budget::tests::frozen();
        assert!(select_for_provider(&report, &provider).is_some());
        provider.model = "different-model".into();
        assert!(select_for_provider(&report, &provider).is_none());
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("source");
        crate::harness_engineering::tests::source(&root);
        let root = root.canonicalize().unwrap();
        let group = &mut report.groups[0];
        group.source_paths = vec!["src/bin/ca.rs".into()];
        assert!(selected_source(&root, group).is_empty());
        group.workspace_hash = format!("{:x}", Sha256::digest(root.to_str().unwrap().as_bytes()));
        assert_eq!(selected_source(&root, group), group.source_paths);
        std::fs::write(root.join("src/bin/ca.rs"), "// later user work").unwrap();
        let unchanged = root.join("src/bin/codex-team.rs");
        let content = std::fs::read(&unchanged).unwrap();
        std::fs::write(&unchanged, content).unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        assert!(selected_source(&root, group).is_empty());
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    }

    #[test]
    fn hrs253_discovery_reserves_an_independent_root_when_capping_events() {
        let mut records = records();
        let build = records[0].harness.build.clone().unwrap();
        records[0].started_at = 200;
        records[1].started_at = 100;
        let pair = records[0].events[..2].to_vec();
        records[0].events.clear();
        for attempt in 0..80 {
            for (offset, original) in pair.iter().enumerate() {
                let mut event = original.clone();
                event.sequence = attempt * 2 + offset as u64;
                event.id = format!("{}:{}", records[0].task_id, event.sequence);
                event.call_id = Some(format!("call-{attempt}"));
                records[0].events.push(event);
            }
        }
        let selected = select(&records, &BTreeSet::new(), &build).unwrap();
        assert_eq!(selected[0].evidence_ids.len(), 64);
        assert_eq!(selected[0].independent_tasks, 2);
        records.reverse();
        assert_eq!(
            select(&records, &BTreeSet::new(), &build).unwrap(),
            selected
        );
    }

    #[test]
    fn hrs253_discovery_reads_bounded_regular_outcomes_without_following_links() {
        let temp = tempfile::tempdir().unwrap();
        let records = records();
        for record in &records {
            crate::session_outcome::save_outcome(record, temp.path()).unwrap();
        }
        let private = temp.path().join("synthetic-private");
        std::fs::write(&private, "SYNTHETIC_PRIVATE_SENTINEL").unwrap();
        std::os::unix::fs::symlink(private, temp.path().join("outcomes/linked.json")).unwrap();
        let report = discover(temp.path(), None).unwrap();
        assert_eq!(report.scanned_outcomes, 2);
        assert_eq!(report.unreadable_outcomes, 1);
        assert_eq!(report.groups.len(), 1);
        let vault = LearningVault::open_at(temp.path().join("vault")).unwrap();
        let (mut plan, _, _) = crate::harness_generation::tests::inputs();
        plan.evidence_ids = report.groups[0].evidence_ids.clone();
        plan.seal().unwrap();
        codex_team::self_improvement::start_harness_cycle(&vault, &plan, 0).unwrap();
        assert!(discover(temp.path(), Some(&vault))
            .unwrap()
            .groups
            .is_empty());
        let oversized = std::fs::File::create(temp.path().join("outcomes/large.json")).unwrap();
        oversized.set_len(2 * 1024 * 1024 + 1).unwrap();
        assert_eq!(discover(temp.path(), None).unwrap().unreadable_outcomes, 2);
    }
}
