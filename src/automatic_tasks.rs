//! Ordinary interactive task admission uses the existing durable plan/graph.
use crate::agent_graph::{AgentGraphBudgetInput, AgentTaskInput};
use crate::work::{WorkMutation, WorkState};
use schemars::JsonSchema;
use serde::Deserialize;

pub const GUIDANCE: &str = "Use the supplied context first. Inspect only what is needed to define the requested task map. Once the requested operation and target are clear, return the map without unrelated exploration. This is an automatic task map for host validation. Return graph as well as title/body/tasks. Keep one task if splitting adds no value. Each graph task_id, depends_on entry and consumes.producer must use an EXACT task title from tasks (the host assigns UUIDs). Include every task exactly once. Each task must explicitly provide tools (tool names such as read_file, write_file, edit_file, run_command), paths (workspace-relative grants), commands (permitted command strings), and evidence (completion gates). Workers cannot read, write or run commands without those grants; use empty arrays only when the task needs no such access. Task paths cover only workspace files/directories; external command executables and profiles belong in commands, not path grants. Handoff produces/consumes names use lowercase letters, digits and underscores only (for example preservation_analysis). Declare narrow supported tools, paths and commands. For inspection-only tasks, use tool_success evidence for the required native tool; use artifact or closed_command evidence to verify resulting state. Assistant output alone cannot prove completion. Use human_review for judgments without a mechanical verifier. Order conflicting writers in the same workspace through dependencies. Preserve all shared purpose/constraints. All graph workers share a fixed 100,000 cumulative input/output token ceiling, including repeated input history on every request. Keep task briefs and evidence reads narrow; this ceiling is not per worker and cannot be increased by the graph proposal. Include a finite graph budget; use allow_unpriced=true and max_physical_requests=30 when no monetary ceiling was supplied, and disclose that request cap in the plan. Do not execute. The host starts the validated graph only if the originating Send and run action authorized automatic execution; otherwise the user must approve it before it starts.";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphProposal {
    pub budget: AgentGraphBudgetInput,
    pub tasks: Vec<AgentTaskInput>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ResultSchema {
    title: String,
    body: String,
    tasks: Vec<String>,
    graph: GraphProposal,
}

pub fn output_schema() -> serde_json::Value {
    // Keep the actual grant and evidence shape visible in the model-facing tool
    // schema. Generic/manual graph inputs retain their compatible serde defaults.
    let mut settings = schemars::gen::SchemaSettings::draft07();
    settings.inline_subschemas = true;
    let mut schema = serde_json::to_value(
        settings
            .into_generator()
            .into_root_schema_for::<ResultSchema>(),
    )
    .expect("task map schema is serializable");
    let required = schema
        .pointer_mut("/properties/graph/properties/tasks/items/required")
        .and_then(serde_json::Value::as_array_mut)
        .expect("automatic graph tasks have required fields");
    required.extend(["tools", "paths", "commands", "evidence"].map(serde_json::Value::from));
    schema
}

pub fn output_contract() -> Result<crate::headless_contract::StructuredOutputContract, String> {
    Ok(
        crate::headless_contract::StructuredOutputContract::native_automatic_task_map()?
            .with_value_validator(preflight),
    )
}

/// Graph locations are workspace-relative host data, unlike prose, whose exact
/// authored spelling is restored for display. Resolve issued references while
/// their provider spelling still identifies the admitted path.
pub(crate) fn restore_graph_paths(
    privacy: &crate::privacy::Context,
    provider_value: &serde_json::Value,
    restored: &mut serde_json::Value,
) -> Result<(), String> {
    let _ = (privacy, provider_value, restored);
    Ok(())
}

fn preflight(value: &serde_json::Value) -> Result<(), String> {
    let proposal: ResultSchema =
        serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    // Use exactly the execution configuration checks on disposable memory. This
    // reserves no budget, writes no state and grants no execution authority.
    let mut work = WorkState::default();
    work.apply_model_mutation(WorkMutation::Propose {
        expected_revision: 0,
        title: proposal.title,
        body: Some(proposal.body),
        tasks: proposal.tasks,
    })
    .map_err(|error| format!("Task map preflight failed: {error}"))?;
    configure(&mut work, proposal.graph).map_err(|error| {
        format!("Task map preflight failed: {error}. Handoff names must use lowercase letters, digits and underscores; task paths must be workspace-relative, with external executables/profiles only in commands. Keep grants, dependencies, budget and evidence within the declared task contract.")
    })
}

pub fn configure(work: &mut WorkState, mut graph: GraphProposal) -> Result<(), String> {
    let plan = work.plan.as_ref().ok_or("No task map plan")?;
    if plan
        .tasks
        .iter()
        .any(|task| crate::task_scope::plan_context(plan, task.id).is_none())
    {
        return Err("Automatic task maps require complete shared constraints and one focused section per task".into());
    }
    let ids = plan
        .tasks
        .iter()
        .map(|task| (task.title.clone(), task.id.to_string()))
        .collect::<std::collections::HashMap<_, _>>();
    if ids.len() != plan.tasks.len() {
        return Err("Task map titles must be unique".into());
    }
    let resolve = |name: &str| {
        ids.get(name)
            .cloned()
            .ok_or_else(|| format!("Unknown task title: {name}"))
    };
    for task in &mut graph.tasks {
        task.task_id = resolve(&task.task_id)?;
        task.depends_on = task
            .depends_on
            .iter()
            .map(|name| resolve(name))
            .collect::<Result<_, _>>()?;
        for input in &mut task.consumes {
            input.producer = resolve(&input.producer)?;
        }
    }
    // Existing shared-workspace lanes serialize writers. Derive these edges
    // before review rather than asking the user to repair an unsafe proposal.
    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while ordered.len() < graph.tasks.len() {
        let ready = graph
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, task)| {
                !seen.contains(&task.task_id) && task.depends_on.iter().all(|id| seen.contains(id))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if ready.is_empty() {
            return Err("Task map contains duplicate tasks or cyclic dependencies".into());
        }
        for index in ready {
            seen.insert(graph.tasks[index].task_id.clone());
            ordered.push(index);
        }
    }
    let registry = crate::agent::registry::ToolRegistry::builtins();
    let mut mutating = registry
        .specs()
        .iter()
        .filter(|spec| spec.mutation == crate::agent::registry::ToolMutation::Mutating)
        .map(|spec| spec.name.as_str().to_owned())
        .collect::<std::collections::HashSet<_>>();
    mutating.insert("run_command".into());
    let mut previous_writer = std::collections::HashMap::new();
    for index in ordered {
        let task = &mut graph.tasks[index];
        if task.tools.iter().any(|tool| mutating.contains(tool)) {
            if let Some(previous) =
                previous_writer.insert(task.worktree_id.clone(), task.task_id.clone())
            {
                if !task.depends_on.contains(&previous) {
                    task.depends_on.push(previous);
                }
            }
        }
    }
    work.apply_model_mutation(WorkMutation::ConfigureAgentGraph {
        expected_revision: work.revision(),
        budget: graph.budget,
        tasks: graph.tasks,
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// A worker-owned save baseline; each reservation is durable before dispatch.
pub struct GraphPersistence {
    pub store: crate::session::SessionStore,
    pub session: crate::session::SessionV2,
    pub lease: Option<crate::session_access::SessionWriterLease>,
}

impl GraphPersistence {
    pub async fn save(&mut self, work: WorkState) -> Result<(), String> {
        let mut candidate = self.session.clone();
        candidate.work_state = work;
        let store = self.store.clone();
        let lease = self.lease.clone();
        let candidate = tokio::task::spawn_blocking(move || {
            let context = crate::privacy::for_session(candidate.id);
            let save = || {
                save_reconciled(&store, candidate, |candidate| {
                    match lease.as_ref() {
                        Some(lease) => store.save_owned(candidate, lease),
                        None => store.save(candidate),
                    }
                    .map(|_| ())
                })
            };
            match context {
                Some(context) => context.with_session_save(save),
                None => save(),
            }
        })
        .await
        .map_err(|error| error.to_string())?;
        self.session = candidate?;
        Ok(())
    }
}

/// A late worker result must not replace an intervening TUI save baseline.
pub fn adopt_save_baseline(
    current: &mut Option<crate::session::SessionV2>,
    saved: crate::session::SessionV2,
) -> bool {
    let adopt = current.as_ref().is_some_and(|current| {
        current.id == saved.id
            && match (&current.storage_binding, &saved.storage_binding) {
                (None, Some(_)) => true,
                (Some(current), Some(saved)) => {
                    current == saved
                        || ((current.storage_generation == saved.storage_generation
                            || current.storage_generation.is_none())
                            && current.metadata_revision < saved.metadata_revision)
                }
                _ => false,
            }
    });
    if adopt {
        *current = Some(saved);
    }
    adopt
}

pub fn same_saved_state(
    actual: &crate::session::SessionV2,
    expected: &crate::session::SessionV2,
) -> bool {
    let metadata = |value: &crate::session::SessionV2| {
        let mut value = serde_json::to_value(value).expect("session serializes");
        value
            .as_object_mut()
            .expect("session object")
            .remove("updated_at");
        value
    };
    metadata(actual) == metadata(expected)
}

fn save_reconciled(
    store: &crate::session::SessionStore,
    mut candidate: crate::session::SessionV2,
    save: impl FnOnce(&mut crate::session::SessionV2) -> std::io::Result<()>,
) -> Result<crate::session::SessionV2, String> {
    match save(&mut candidate) {
        Ok(()) => Ok(candidate),
        Err(error) => match store.load(&candidate.id.to_string()) {
            Ok(actual) if same_saved_state(&actual, &candidate) => Ok(actual),
            _ => Err(error.to_string()),
        },
    }
}

/// A completed, configured ordinary task, with live Send authorization but no
/// graph approval or dispatch. Shared by the execution-boundary tests.
#[cfg(test)]
pub(crate) fn reviewed_execution_fixture() -> WorkState {
    use crate::agent::subagent::AgentRoute;
    use crate::plan_workflow::{
        self, PlanWorkflowArgs, WorkflowParticipantInput, WorkflowRole, WorkflowStep,
    };
    let mut work = WorkState::default();
    let route = AgentRoute {
        provider: "test".into(),
        model: "test".into(),
    };
    plan_workflow::apply_action(
        &mut work,
        PlanWorkflowArgs::Start {
            goal: "Inspect the supplied local evidence".into(),
            participants: vec![WorkflowParticipantInput {
                id: "author".into(),
                route: Some(route.clone()),
            }],
            steps: vec![WorkflowStep {
                role: WorkflowRole::Author,
                participant: "author".into(),
                route: None,
                instructions: String::new(),
                evidence: None,
            }],
            review_loop: None,
            implementation: None,
        },
        &route,
        None,
    )
    .unwrap();
    work.automatic_task_map = true;
    work.authorize_automatic_task_execution().unwrap();
    assert!(work.ready_automatic_task_execution().is_none());
    let dispatch = plan_workflow::next_dispatch(&work).unwrap().unwrap();
    let id = uuid::Uuid::new_v4();
    plan_workflow::mark_dispatched(&mut work, &dispatch, id).unwrap();
    plan_workflow::complete_stage(&mut work, id, serde_json::json!({
        "title": "Inspect evidence", "body": "## Shared purpose and constraints\nInspect local evidence.\n## Task: Inspect\nRead evidence.txt.\n",
        "tasks": ["Inspect"],
        "graph": {"budget": {"allow_unpriced": true, "max_physical_requests": 30}, "tasks": [{"task_id":"Inspect", "tools":["read_file"], "paths":["evidence.txt"], "commands":[], "evidence":[{"kind":"human_review"}]}]}
    })).unwrap();
    work
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_map_restores_typed_paths_without_rewriting_prose_or_granting_unknown_paths() {
        let filter = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&filter, || {
            let temporary = tempfile::tempdir().unwrap();
            let workspace = temporary.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            std::fs::write(workspace.join("private-evidence.txt"), "evidence").unwrap();
            let privacy = crate::privacy::Context::open(
                temporary.path().join("profile"),
                workspace.clone(),
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            privacy.allow_ephemeral();
            privacy.refresh_filenames().unwrap();
            let root = privacy.project_filename_path(&workspace).unwrap();
            let file = privacy
                .project_filename_path(&workspace.join("private-evidence.txt"))
                .unwrap();
            let value = serde_json::json!({
                "body": format!("List {root}, then read {file}."),
                "graph": {"tasks": [{
                    "paths": [root, file, "public-output.txt"],
                    "commands": [],
                    "evidence": [{"kind":"artifact", "path":file}, {"kind":"closed_command", "cwd":root}],
                    "produces": [{"path":file}], "consumes": [{"path":file}]
                }]}
            });
            let mut restored = privacy.restore_arguments(&value).unwrap();
            let prose = restored["body"].clone();
            restore_graph_paths(&privacy, &value, &mut restored).unwrap();
            assert_eq!(restored["body"], prose);
            assert_eq!(
                restored.pointer("/graph/tasks/0/paths").unwrap(),
                &serde_json::json!([".", "private-evidence.txt", "public-output.txt"])
            );
            assert_eq!(
                restored.pointer("/graph/tasks/0/evidence/0/path").unwrap(),
                "private-evidence.txt"
            );
            assert_eq!(
                restored.pointer("/graph/tasks/0/evidence/1/cwd").unwrap(),
                "."
            );
            assert_eq!(
                restored.pointer("/graph/tasks/0/produces/0/path").unwrap(),
                "private-evidence.txt"
            );
            assert_eq!(
                restored.pointer("/graph/tasks/0/consumes/0/path").unwrap(),
                "private-evidence.txt"
            );
            let unchanged = restored.clone();
            let mut invalid = value.clone();
            invalid["graph"]["tasks"][0]["paths"][1] = "not-issued".into();
            assert!(restore_graph_paths(&privacy, &invalid, &mut restored).is_err());
            assert_eq!(
                restored, unchanged,
                "failed normalization must be transactional"
            );
            let outside = temporary.path().join("outside");
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("external.txt"), "external").unwrap();
            let roots = crate::workspace::WorkspaceContext::from_roots(
                &workspace,
                std::slice::from_ref(&outside),
            )
            .unwrap();
            privacy.set_tool_roots(&roots);
            privacy.refresh_filenames().unwrap();
            let external = privacy
                .project_filename_path(&outside.join("external.txt"))
                .unwrap();
            invalid["graph"]["tasks"][0]["paths"][1] = external.into();
            assert!(restore_graph_paths(&privacy, &invalid, &mut restored).is_err());
            assert_eq!(
                restored, unchanged,
                "another granted root is not this task's workspace"
            );
            std::fs::remove_file(workspace.join("private-evidence.txt")).unwrap();
            privacy.refresh_filenames().unwrap();
            assert!(restore_graph_paths(&privacy, &value, &mut restored).is_err());
            assert_eq!(restored, unchanged);
            assert!(output_contract().unwrap().is_native_automatic_task_map());
            assert!(
                !crate::headless_contract::StructuredOutputContract::from_value(output_schema())
                    .unwrap()
                    .is_native_automatic_task_map()
            );
        });
    }

    #[test]
    fn automatic_map_native_root_inspection_has_authoritative_completion() {
        let value = serde_json::json!({
            "title":"List the workspace",
            "body":"## Shared purpose and constraints\nList the workspace without changing it.\n## Task: List\nUse list_dir on the current workspace.\n",
            "tasks":["List"],
            "graph":{"budget":{"allow_unpriced":true,"max_physical_requests":12},
                "tasks":[{"task_id":"List","tools":["list_dir"],"paths":["."],"commands":[],
                    "evidence":[{"kind":"tool_success","tool":"list_dir"}]}]}
        });
        output_contract().unwrap().validate_value(&value).unwrap();
        let mut missing_tool = value;
        missing_tool["graph"]["tasks"][0]["tools"] = serde_json::json!([]);
        assert!(output_contract()
            .unwrap()
            .validate_value(&missing_tool)
            .is_err());
    }

    #[test]
    fn automatic_task_send_is_exact_one_use_and_separate_from_manual_approval() {
        let mut work = reviewed_execution_fixture();
        let review = work.ready_automatic_task_execution().unwrap();
        assert!(work.automatic_task_execution_matches(review));
        assert!(work
            .approve_automatic_task_execution(crate::work::PlanReview {
                revision: review.revision + 1,
                ..review
            })
            .is_err());
        assert!(work.automatic_task_execution_matches(review));
        work.approve_automatic_task_execution(review).unwrap();
        assert_eq!(
            work.plan.as_ref().unwrap().approved_by.as_deref(),
            Some("automatic_task_send")
        );
        assert!(!work.automatic_task_execution_authorized());
        assert!(work.approve_automatic_task_execution(review).is_err());
        work.user_start_agent_graph().unwrap();
        assert_eq!(
            work.agent_graph.status,
            crate::agent_graph::AgentGraphStatus::Running
        );
    }

    #[test]
    fn automatic_task_send_does_not_survive_restore_fork_revisions_or_manual_choice() {
        let mut work = reviewed_execution_fixture();
        let review = work.ready_automatic_task_execution().unwrap();
        let decoded: WorkState =
            serde_json::from_value(serde_json::to_value(&work).unwrap()).unwrap();
        assert!(!decoded.automatic_task_execution_authorized());
        assert!(!work.fork_full().automatic_task_execution_authorized());
        let mut restored = work.clone();
        restored.pause_agent_graph_after_restore(chrono::Utc::now());
        assert!(!restored.automatic_task_execution_authorized());
        let mut edited = work.clone();
        edited
            .apply_model_mutation(WorkMutation::Edit {
                expected_revision: edited.revision(),
                title: Some("Changed".into()),
                body: None,
                decisions: Vec::new(),
            })
            .unwrap();
        assert!(!edited.automatic_task_execution_authorized());
        work.user_decide(review.revision, false).unwrap();
        assert!(!work.automatic_task_execution_authorized());
        assert!(work.plan.as_ref().unwrap().approved_by.is_none());
    }

    #[test]
    fn automatic_task_send_cannot_transfer_to_another_workflow() {
        let mut work = reviewed_execution_fixture();
        work.plan_workflow.as_mut().unwrap().id = uuid::Uuid::new_v4();
        assert!(work.ready_automatic_task_execution().is_none());
        let mut explicit = reviewed_execution_fixture();
        explicit.automatic_task_map = false;
        assert!(explicit.ready_automatic_task_execution().is_none());
        assert!(explicit.authorize_automatic_task_execution().is_err());
    }

    #[test]
    fn automatic_map_discloses_fixed_shared_token_budget() {
        let input = serde_json::from_value(
            serde_json::json!({"allow_unpriced":true,"max_physical_requests":30}),
        )
        .unwrap();
        let limits = crate::agent_graph::AgentGraphBudgetLimitsV1::from_input(input).unwrap();
        assert_eq!(limits.max_total_tokens, 100_000);
        assert!(GUIDANCE.contains("fixed 100,000 cumulative input/output token ceiling"));
        assert!(GUIDANCE.contains("not per worker"));
        let contract =
            crate::headless_contract::StructuredOutputContract::from_value(output_schema())
                .unwrap();
        let raised = serde_json::json!({"title":"Map","body":"Plan","tasks":[],"graph":{"budget":{"allow_unpriced":true,"max_physical_requests":30,"max_total_tokens":200000},"tasks":[]}});
        assert!(contract.validate_value(&raised).is_err());
    }

    #[test]
    fn automatic_map_late_worker_save_does_not_replace_newer_baseline() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(root.path().join("sessions"));
        let mut old =
            crate::session::SessionV2::new("Map", "/workspace", "test", "model", Vec::new());
        store.save(&mut old).unwrap();
        let mut newer = old.clone();
        newer.title = "Newer TUI save".into();
        store.save(&mut newer).unwrap();
        let mut current = Some(newer.clone());
        assert!(!adopt_save_baseline(&mut current, old));
        assert_eq!(current.unwrap().title, "Newer TUI save");
        let mut current = None;
        assert!(!adopt_save_baseline(&mut current, newer));
    }

    #[test]
    fn automatic_map_save_reconciles_only_identical_committed_state() {
        let root = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(root.path().join("sessions"));
        let candidate =
            crate::session::SessionV2::new("Map", "/workspace", "test", "model", Vec::new());
        assert!(
            save_reconciled(&store, candidate.clone(), |_| Err(std::io::Error::other(
                "before commit"
            )))
            .is_err()
        );
        assert!(store.load(&candidate.id.to_string()).is_err());
        let committed = save_reconciled(&store, candidate, |candidate| {
            store.save(candidate)?;
            Err(std::io::Error::other("after commit"))
        })
        .unwrap();
        let mut changed = committed.clone();
        changed.title = "different local state".into();
        assert!(
            save_reconciled(&store, changed, |_| Err(std::io::Error::other(
                "receipt error"
            )))
            .is_err()
        );
        assert_eq!(store.load(&committed.id.to_string()).unwrap().title, "Map");
    }

    #[test]
    fn automatic_map_schema_rejects_invalid_graph_before_handoff() {
        let contract =
            crate::headless_contract::StructuredOutputContract::from_value(output_schema())
                .unwrap();
        let mut value = serde_json::json!({"title":"Map","body":"Plan","tasks":[],"graph":{"budget":{"allow_unpriced":true,"max_physical_requests":24},"tasks":[],"approval_required":true}});
        assert!(contract.validate_value(&value).is_err());
        value["graph"]
            .as_object_mut()
            .unwrap()
            .remove("approval_required");
        assert!(contract.validate_value(&value).is_ok());
        value["graph"]["tasks"] =
            serde_json::json!([{"task_id":"Inspect","completion_evidence":[]}]);
        assert!(contract.validate_value(&value).is_err());
    }

    #[test]
    fn automatic_map_schema_exposes_and_requires_explicit_grants() {
        let schema = output_schema();
        assert!(schema.get("definitions").is_none());
        let properties = schema
            .pointer("/properties/graph/properties/tasks/items/properties")
            .unwrap();
        assert!(properties["evidence"]["items"]["oneOf"].is_array());
        let contract =
            crate::headless_contract::StructuredOutputContract::from_value(schema).unwrap();
        let mut value = serde_json::json!({"title":"Map","body":"Plan","tasks":["Inspect"],"graph":{"budget":{"allow_unpriced":true,"max_physical_requests":24},"tasks":[{"task_id":"Inspect","tools":["read_file"],"paths":["evidence.txt"],"commands":[],"evidence":[{"kind":"human_review"}]}]}});
        assert!(contract.validate_value(&value).is_ok());
        for field in ["tools", "paths", "commands", "evidence"] {
            let mut missing = value.clone();
            missing["graph"]["tasks"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(contract.validate_value(&missing).is_err(), "{field}");
        }
        // An explicitly tool-free judgment remains a valid proposal for review.
        value["graph"]["tasks"][0]["tools"] = serde_json::json!([]);
        value["graph"]["tasks"][0]["paths"] = serde_json::json!([]);
        assert!(contract.validate_value(&value).is_ok());
    }

    #[test]
    fn automatic_map_preflight_checks_execution_policy_before_handoff() {
        let mut value = serde_json::json!({"title":"Map","body":"## Shared purpose and constraints\nKeep local.\n## Task: Inspect\nRead evidence.\n","tasks":["Inspect"],"graph":{"budget":{"allow_unpriced":true,"max_physical_requests":24},"tasks":[{"task_id":"Inspect","tools":["read_file","write_file"],"paths":["evidence.txt","report.txt"],"commands":[],"evidence":[{"kind":"human_review"}],"produces":[{"name":"bad-name","path":"report.txt","kind":"regular_file"}]}]}});
        let plain = crate::headless_contract::StructuredOutputContract::from_value(output_schema())
            .unwrap();
        assert!(plain.validate_value(&value).is_ok());
        let contract = output_contract().unwrap();
        let error = contract.validate_value(&value).unwrap_err();
        assert!(error.contains("Task map preflight failed"));
        value["graph"]["tasks"][0]["produces"][0]["name"] = "valid_name".into();
        assert!(contract.validate_value(&value).is_ok());
        value["graph"]["tasks"][0]["paths"][0] = "/external/helper".into();
        assert!(contract.validate_value(&value).is_err());
        value["graph"]["tasks"][0]["paths"][0] = "evidence.txt".into();
        value["graph"]["tasks"][0]["depends_on"] = serde_json::json!(["Missing"]);
        assert!(contract.validate_value(&value).is_err());
    }

    #[test]
    fn automatic_map_configures_ids_and_rejects_unknown_dependencies() {
        let mut work = WorkState::default();
        work.apply_model_mutation(WorkMutation::Propose {
            expected_revision: 0,
            title: "Map".into(),
            body: Some("## Shared purpose and constraints\nKeep local.\n## Task: Inspect\nRead evidence.\n## Task: Document\nWrite findings.\n".into()),
            tasks: vec!["Inspect".into(), "Document".into()],
        })
        .unwrap();
        let graph = |dependency| {
            serde_json::from_value(serde_json::json!({
            "budget":{"allow_unpriced":true,"max_physical_requests":30},
            "tasks":[
                {"task_id":"Inspect","tools":["read_file"],"paths":["."],"evidence":[{"kind":"human_review"}]},
                {"task_id":"Document","depends_on":[dependency],"tools":["read_file"],"paths":["."],"evidence":[{"kind":"human_review"}]}
            ]
        })).unwrap()
        };
        let before = work.clone();
        let mut ambiguous = work.clone();
        ambiguous.plan.as_mut().unwrap().body = Some("Unstructured full plan".into());
        assert!(configure(&mut ambiguous, graph("Inspect")).is_err());
        assert!(configure(&mut work, graph("Missing")).is_err());
        assert_eq!(work, before);
        configure(&mut work, graph("Inspect")).unwrap();
        let tasks = &work.plan.as_ref().unwrap().tasks;
        assert_eq!(tasks[1].agent.as_ref().unwrap().depends_on, [tasks[0].id]);
        assert!(work.agent_graph.approved_revision.is_none());
    }
}
