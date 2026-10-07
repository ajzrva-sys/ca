//! Process-owned schema discovery. The caller first resolves all authorization
//! constraints; this module only narrows that immutable eligible set.
use crate::agent::client::ToolDef;
use crate::agent::registry::ToolExecutionResult;
use crate::config::ToolDiscoveryMode;
use crate::tool_selection::ToolSelectionPolicy;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub const SEARCH_TOOLS_NAME: &str = "search_tools";
pub const MAX_ACTIVE_EXTENDED: usize = 24;
const MAX_QUERY_BYTES: usize = 256;
const MAX_RESULTS: usize = 8;
const MAX_SCHEMA_BYTES: usize = 32 * 1024;
const CORE: &[&str] = &[
    "read_file",
    "list_dir",
    "search_files",
    "glob",
    "write_file",
    "edit_file",
    "apply_patch",
    "hashline_edit",
    "run_command",
    "request_user_input",
    "read_todos",
    "update_todos",
    "read_scratch",
    "session_search",
    SEARCH_TOOLS_NAME,
];

#[derive(Default)]
struct DiscoveryInner {
    authorized: Vec<ToolDef>,
    active: HashMap<String, u64>,
    sequence: u64,
    batch_id: u64,
    pending: Option<Vec<String>>,
}

#[derive(Clone, Default)]
pub struct ToolDiscoverySession(Arc<Mutex<DiscoveryInner>>);

impl std::fmt::Debug for ToolDiscoverySession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        formatter
            .debug_struct("ToolDiscoverySession")
            .field("active_count", &inner.active.len())
            .field("batch_in_flight", &inner.pending.is_some())
            .finish()
    }
}

impl ToolDiscoverySession {
    /// `authorized` must already include profile, CLI, surface, grants and
    /// capability filters. Eager mode and exact replacement lists are unchanged.
    pub fn advertise(
        &self,
        authorized: Vec<ToolDef>,
        policy: &ToolSelectionPolicy,
    ) -> Vec<ToolDef> {
        let mut inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let authorized_names = authorized
            .iter()
            .map(|tool| tool.function.name.clone())
            .collect::<HashSet<_>>();
        inner
            .active
            .retain(|name, _| authorized_names.contains(name));
        inner.authorized = authorized.clone();
        if policy.discovery_mode() == ToolDiscoveryMode::Eager
            || policy.has_explicit_replacement()
            || !authorized_names.contains(SEARCH_TOOLS_NAME)
        {
            return authorized;
        }
        let pinned = policy.pinned_names().collect::<HashSet<_>>();
        authorized
            .into_iter()
            .filter(|tool| {
                let name = tool.function.name.as_str();
                CORE.contains(&name)
                    || pinned.contains(name)
                    || name.starts_with("mcp__")
                    || name.starts_with("report_")
                    || inner.active.contains_key(name)
            })
            .collect()
    }

    /// A batch is committed only after every tool result is appended. Dropping
    /// this guard on cancellation/protocol failure discards all activation deltas.
    pub fn begin_batch(&self) -> DiscoveryBatch {
        let mut inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        inner.batch_id = inner.batch_id.wrapping_add(1);
        inner.pending = Some(Vec::new());
        DiscoveryBatch {
            session: self.clone(),
            id: inner.batch_id,
            committed: false,
        }
    }

    pub fn touch(&self, name: &str) {
        let mut inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if inner.active.contains_key(name) {
            if let Some(pending) = inner.pending.as_mut() {
                pending.push(name.to_string());
            }
        }
    }

    pub fn search(&self, arguments: &Value) -> ToolExecutionResult {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Search {
            query: String,
            limit: Option<usize>,
        }
        let Ok(request) = serde_json::from_value::<Search>(arguments.clone()) else {
            return ToolExecutionResult::Failure(
                "Tool discovery requires query and optional limit".into(),
            );
        };
        let query = request.query.trim();
        let limit = request.limit.unwrap_or(4);
        if query.is_empty()
            || query.len() > MAX_QUERY_BYTES
            || query.chars().any(char::is_control)
            || !(1..=MAX_RESULTS).contains(&limit)
        {
            return ToolExecutionResult::Failure(
                "Tool discovery requires a query of at most 256 bytes and limit 1-8".into(),
            );
        }
        let mut inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if inner.pending.is_none() {
            return ToolExecutionResult::Failure(
                "Tool discovery requires an owned active tool batch".into(),
            );
        }
        let query = query.to_ascii_lowercase();
        let terms = query.split_whitespace().collect::<Vec<_>>();
        let mut ranked = inner
            .authorized
            .iter()
            .filter_map(|tool| {
                let name = &tool.function.name;
                if name == SEARCH_TOOLS_NAME || name.starts_with("mcp__") {
                    return None;
                }
                let folded_name = name.to_ascii_lowercase();
                let description = tool.function.description.to_ascii_lowercase();
                let exact = usize::from(folded_name == query);
                let name_hits = terms
                    .iter()
                    .filter(|term| folded_name.contains(**term))
                    .count();
                let description_hits = terms
                    .iter()
                    .filter(|term| description.contains(**term))
                    .count();
                let score = exact * 10_000 + name_hits * 100 + description_hits;
                (score > 0).then_some((score, tool.clone()))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.function.name.cmp(&right.1.function.name))
        });
        let mut definitions = Vec::new();
        for (_, definition) in ranked.into_iter().take(limit) {
            definitions.push(definition);
            if serde_json::to_vec(&definitions).map_or(true, |bytes| bytes.len() > MAX_SCHEMA_BYTES)
            {
                definitions.pop();
            }
        }
        let names = definitions
            .iter()
            .map(|tool| tool.function.name.clone())
            .collect::<Vec<_>>();
        inner
            .pending
            .as_mut()
            .expect("owned batch checked")
            .extend(names.clone());
        ToolExecutionResult::Success(serde_json::json!({
            "notice": "Matching tools become advertised on the next request after this entire tool batch completes; they cannot be called in the current batch.",
            "activated_on_next_request": names, "tools": definitions,
        }).to_string())
    }

    #[cfg(test)]
    pub fn reset(&self) {
        let mut inner = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let next_batch_id = inner.batch_id.wrapping_add(1);
        *inner = DiscoveryInner {
            batch_id: next_batch_id,
            ..DiscoveryInner::default()
        };
    }
}

#[derive(Debug)]
pub struct DiscoveryBatch {
    session: ToolDiscoverySession,
    id: u64,
    committed: bool,
}

impl DiscoveryBatch {
    pub fn commit(mut self) {
        let mut inner = self
            .session
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if inner.batch_id == self.id {
            if let Some(pending) = inner.pending.take() {
                for name in pending {
                    if CORE.contains(&name.as_str()) {
                        continue;
                    }
                    inner.sequence = inner.sequence.wrapping_add(1);
                    let sequence = inner.sequence;
                    inner.active.insert(name, sequence);
                }
                let mut recency = inner
                    .active
                    .iter()
                    .map(|(name, stamp)| (name.clone(), *stamp))
                    .collect::<Vec<_>>();
                recency
                    .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
                for (name, _) in recency.into_iter().skip(MAX_ACTIVE_EXTENDED) {
                    inner.active.remove(&name);
                }
            }
        }
        self.committed = true;
    }
}

impl Drop for DiscoveryBatch {
    fn drop(&mut self) {
        if !self.committed {
            let mut inner = self
                .session
                .0
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if inner.batch_id == self.id {
                inner.pending = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::client::ToolFunction;
    fn tool(name: &str) -> ToolDef {
        ToolDef {
            def_type: "function".into(),
            function: ToolFunction {
                name: name.into(),
                description: format!("Test capability {name}"),
                parameters: serde_json::json!({"type":"object"}),
            },
        }
    }
    fn policy() -> ToolSelectionPolicy {
        ToolSelectionPolicy::default().with_discovery_mode(ToolDiscoveryMode::Lazy)
    }
    fn names(tools: &[ToolDef]) -> Vec<&str> {
        tools
            .iter()
            .map(|tool| tool.function.name.as_str())
            .collect()
    }
    #[test]
    fn representative_main_and_child_schema_bytes_drop_at_least_thirty_percent() {
        use crate::agent::registry::{ToolProfile, ToolRegistry};
        for profile in [ToolProfile::Main, ToolProfile::Subagent] {
            let policy = policy();
            let constraints = crate::tool_selection::ToolSelectionConstraints {
                automation_proposals_enabled: profile == ToolProfile::Main,
                ..Default::default()
            };
            let full = crate::tool_selection::resolve_tools(
                ToolRegistry::builtins(),
                profile,
                &policy,
                &constraints,
            )
            .unwrap()
            .into_definitions();
            let mut lazy = ToolDiscoverySession::default().advertise(full, &policy);
            let mut eager = crate::tool_selection::resolve_tools(
                ToolRegistry::builtins(),
                profile,
                &ToolSelectionPolicy::default(),
                &constraints,
            )
            .unwrap()
            .into_definitions();
            let default_eager_bytes = serde_json::to_vec(&eager).unwrap().len();
            let default_lazy_bytes = serde_json::to_vec(&lazy).unwrap().len();
            assert!(default_lazy_bytes * 100 <= default_eager_bytes * 70);
            eprintln!("{profile:?} default schema overhead: eager={default_eager_bytes} bytes, lazy={default_lazy_bytes} bytes, saved={:.2}%", (default_eager_bytes - default_lazy_bytes) as f64 * 100.0 / default_eager_bytes as f64);
            let contract = tool("report_structured_output");
            lazy.push(contract.clone());
            eager.push(contract);
            let eager_bytes = serde_json::to_vec(&eager).unwrap().len();
            let lazy_bytes = serde_json::to_vec(&lazy).unwrap().len();
            eprintln!("{profile:?} schema overhead: eager={eager_bytes} bytes, lazy={lazy_bytes} bytes, saved={:.2}%", (eager_bytes - lazy_bytes) as f64 * 100.0 / eager_bytes as f64);
            assert!(
                lazy_bytes * 100 <= eager_bytes * 70,
                "{profile:?}: eager={eager_bytes},lazy={lazy_bytes}"
            );
            assert!(names(&lazy).contains(&"report_structured_output"));
        }
    }
    #[test]
    fn search_only_activates_after_complete_batch_and_current_snapshot_is_frozen() {
        let session = ToolDiscoverySession::default();
        let full = vec![tool("read_file"), tool(SEARCH_TOOLS_NAME), tool("browser")];
        let initial = session.advertise(full.clone(), &policy());
        let batch = session.begin_batch();
        assert!(session
            .search(&serde_json::json!({"query":"browser"}))
            .is_success());
        assert_eq!(names(&initial), ["read_file", SEARCH_TOOLS_NAME]);
        assert!(!names(&session.advertise(full.clone(), &policy())).contains(&"browser"));
        batch.commit();
        assert!(names(&session.advertise(full, &policy())).contains(&"browser"));
    }
    #[test]
    fn cancelled_search_does_not_leak_to_next_turn_or_other_session() {
        let session = ToolDiscoverySession::default();
        let full = vec![tool(SEARCH_TOOLS_NAME), tool("browser")];
        session.advertise(full.clone(), &policy());
        {
            let _batch = session.begin_batch();
            session.search(&serde_json::json!({"query":"browser"}));
        }
        assert_eq!(
            names(&session.advertise(full.clone(), &policy())),
            [SEARCH_TOOLS_NAME]
        );
        assert_eq!(
            names(&ToolDiscoverySession::default().advertise(full, &policy())),
            [SEARCH_TOOLS_NAME]
        );
    }
    #[test]
    fn reset_invalidates_old_guards_without_discarding_new_owned_batch() {
        let session = ToolDiscoverySession::default();
        let full = vec![tool(SEARCH_TOOLS_NAME), tool("browser")];
        session.advertise(full.clone(), &policy());
        let stale = session.begin_batch();
        session.search(&serde_json::json!({"query":"browser"}));
        session.reset();
        session.advertise(full.clone(), &policy());
        let live = session.begin_batch();
        session.search(&serde_json::json!({"query":"browser"}));
        drop(stale);
        live.commit();
        assert!(names(&session.advertise(full, &policy())).contains(&"browser"));
    }
    #[test]
    fn authorization_changes_remove_activation_and_search_results_are_schema_bounded() {
        let session = ToolDiscoverySession::default();
        let mut huge = tool("huge");
        huge.function.description = "huge".repeat(12_000);
        let full = vec![tool(SEARCH_TOOLS_NAME), tool("browser"), huge];
        session.advertise(full.clone(), &policy());
        let batch = session.begin_batch();
        assert!(session
            .search(&serde_json::json!({"query":"huge"}))
            .model_text()
            .contains("\"tools\":[]"));
        session.search(&serde_json::json!({"query":"browser"}));
        batch.commit();
        let narrowed = session.advertise(vec![tool(SEARCH_TOOLS_NAME)], &policy());
        assert_eq!(names(&narrowed), [SEARCH_TOOLS_NAME]);
        let batch = session.begin_batch();
        assert!(session
            .search(&serde_json::json!({"query":"browser"}))
            .model_text()
            .contains("\"tools\":[]"));
        batch.commit();
        assert!(!names(&session.advertise(full, &policy())).contains(&"browser"));
    }
    #[test]
    fn eager_exact_lists_missing_discovery_and_explicit_includes_preserve_authority() {
        let session = ToolDiscoverySession::default();
        let full = vec![tool(SEARCH_TOOLS_NAME), tool("browser"), tool("email_read")];
        assert_eq!(
            names(&session.advertise(full.clone(), &ToolSelectionPolicy::default())),
            names(&full)
        );
        let exact = ToolSelectionPolicy::replacing(&["browser"])
            .with_discovery_mode(ToolDiscoveryMode::Lazy);
        assert_eq!(
            names(&session.advertise(vec![tool("browser")], &exact)),
            ["browser"]
        );
        let include = ToolSelectionPolicy::from_cli(None, vec!["browser".into()], vec![])
            .unwrap()
            .with_discovery_mode(ToolDiscoveryMode::Lazy);
        assert_eq!(
            names(&session.advertise(full, &include)),
            [SEARCH_TOOLS_NAME, "browser"]
        );
        let _batch = session.begin_batch();
        let result = session.search(&serde_json::json!({"query":"run_privileged"}));
        assert!(result.is_success());
        assert!(result.model_text().contains("\"tools\":[]"));
    }
    #[test]
    fn search_and_schema_working_set_are_bounded_and_lru_evicted() {
        let session = ToolDiscoverySession::default();
        let full = std::iter::once(tool(SEARCH_TOOLS_NAME))
            .chain((0..40).map(|index| tool(&format!("extra_{index:02}"))))
            .collect::<Vec<_>>();
        session.advertise(full.clone(), &policy());
        assert!(!session
            .search(&serde_json::json!({"query":"extra"}))
            .is_success());
        for index in 0..40 {
            let batch = session.begin_batch();
            assert!(session
                .search(&serde_json::json!({"query":format!("extra_{index:02}"),"limit":1}))
                .is_success());
            batch.commit();
        }
        let advertised = session.advertise(full, &policy());
        assert_eq!(advertised.len(), 25);
        assert!(!names(&advertised).contains(&"extra_00"));
        assert!(names(&advertised).contains(&"extra_39"));
        let _batch = session.begin_batch();
        for args in [
            serde_json::json!({"query":"","limit":4}),
            serde_json::json!({"query":"extra","limit":9}),
            serde_json::json!({"query":"a".repeat(257)}),
            serde_json::json!({"query":"extra","unknown":true}),
        ] {
            assert!(!session.search(&args).is_success());
        }
    }
}
