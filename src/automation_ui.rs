//! Host-owned automation management. Draft confirmation never substitutes model authority.
use crate::automation::{AutomationDefinitionV1, AutomationStore};
use crate::runtime::{ActiveProfile, RuntimePaths};
use std::collections::VecDeque;
use std::path::Path;
use uuid::Uuid;

pub const USAGE: &str = "/automation [list|inspect ID|history [ID]|pause ID|resume ID|run ID|status RUN_UUID|cancel RUN_UUID|reconcile RUN_UUID|result ID [RUN_UUID]|preview FILE|create TOKEN|update ID TOKEN|delete ID [TOKEN]]";

#[derive(Debug, Clone)]
pub(crate) struct AutomationToolScope {
    pub paths: RuntimePaths,
    pub profile: ActiveProfile,
}

pub(crate) async fn list_model_metadata(
    scope: AutomationToolScope,
    arguments: &serde_json::Value,
    timeout_secs: u64,
) -> crate::agent::registry::ToolExecutionResult {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Arguments {
        limit: Option<usize>,
        offset: Option<usize>,
    }
    if arguments.as_object().is_none_or(|object| {
        object.len() > 2
            || object.iter().any(|(key, value)| {
                !matches!(key.as_str(), "limit" | "offset") || !(value.is_null() || value.is_u64())
            })
    }) {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "Automation metadata accepts only bounded numeric limit and offset.".into(),
        );
    }
    let args = match serde_json::from_value::<Arguments>(arguments.clone()) {
        Ok(args)
            if (1..=25).contains(&args.limit.unwrap_or(25)) && args.offset.unwrap_or(0) <= 255 =>
        {
            args
        }
        _ => {
            return crate::agent::registry::ToolExecutionResult::Failure(
                "Automation metadata requires limit 1..25 and offset 0..255.".into(),
            )
        }
    };
    if matches!(scope.profile, ActiveProfile::Bare) {
        return crate::agent::registry::ToolExecutionResult::Failure(
            "Automation metadata requires a persistent profile.".into(),
        );
    }
    static WORKERS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4)));
    let deadline = std::time::Duration::from_secs(timeout_secs.clamp(1, 5));
    let operation = async move {
        let permit = WORKERS
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "Automation metadata worker stopped".to_string())?;
        tokio::task::spawn_blocking(move || {
            // Retain capacity if the request is cancelled while the filesystem
            // worker is still completing its bounded metadata read.
            let _permit = permit;
            let mut records = AutomationStore::for_paths(&scope.paths, &scope.profile).records()
                .map_err(|error| error.to_string())?;
            records.sort_by(|left, right| left.definition.id.cmp(&right.definition.id));
            let total = records.len();
            let offset = args.offset.unwrap_or(0);
            let limit = args.limit.unwrap_or(25);
            let rows = records.into_iter().skip(offset).take(limit).map(|record| {
                let (kind, target_session) = match record.definition.automation_kind {
                    crate::automation::AutomationKindV1::Standalone => ("standalone", None),
                    crate::automation::AutomationKindV1::Heartbeat {session_id, ..} => ("heartbeat", Some(session_id)),
                };
                let state = if record.blocked_reason.is_some()
                    && record.active_run_id.is_some() && record.active_claim.is_some()
                    && record.active_run_id.is_some_and(|run| scope.paths.profile_data_dir(&scope.profile).join("automations/v1/runs").join(run.to_string()).join("delivery.json").is_file()) {"pending_delivery"}
                    else if record.blocked_reason.is_some() {"needs_attention"}
                    else if record.active_run_id.is_some() {"running"}
                    else if record.enabled {"ready"} else {"paused"};
                serde_json::json!({
                    "id": record.definition.id,
                    "name": crate::dashboard_snapshot::bounded_text(&record.definition.name, 128),
                    "kind": kind, "target_session": target_session, "enabled": record.enabled,
                    "state": state,
                    "next_due_at": record.pending_heartbeat.as_ref().or(record.next_occurrence.as_ref()).map(|occurrence| occurrence.at.to_rfc3339()),
                    "coalesced_occurrences": record.coalesced_occurrences,
                })
            }).collect::<Vec<_>>();
            let output = serde_json::to_string(&serde_json::json!({"automations":rows,"total":total,"offset":offset,"truncated":offset.saturating_add(limit)<total}))
                .map_err(|error| error.to_string())?;
            if output.len() > 32 * 1024 { return Err("Automation metadata exceeds the 32KiB output limit".into()); }
            Ok(output)
        }).await.map_err(|_| "Automation metadata worker stopped".to_string())?
    };
    match tokio::time::timeout(deadline, operation).await {
        Ok(Ok(output)) => crate::agent::registry::ToolExecutionResult::Success(output),
        Ok(Err(error)) => crate::agent::registry::ToolExecutionResult::Failure(error),
        Err(_) => crate::agent::registry::ToolExecutionResult::Failure(
            "Automation metadata read timed out".into(),
        ),
    }
}

#[derive(Debug, Clone)]
pub struct AutomationDraft {
    pub definition: AutomationDefinitionV1,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct AutomationDeleteDraft {
    pub id: String,
    pub digest: String,
}

#[derive(Debug, Default)]
pub struct AutomationUiState {
    pub pending: VecDeque<AutomationUiRequest>,
    pub in_flight: usize,
    pub draft: Option<AutomationDraft>,
    delete_draft: Option<AutomationDeleteDraft>,
    latest_preview: Option<Uuid>,
}

#[derive(Debug)]
pub struct AutomationUiRequest {
    pub id: Uuid,
    action: AutomationUiAction,
}

impl AutomationUiRequest {
    pub fn is_run(&self) -> bool {
        matches!(self.action, AutomationUiAction::Run(_))
    }
}

#[derive(Debug)]
enum AutomationUiAction {
    List,
    Inspect(String),
    History(Option<String>),
    Pause(String),
    Resume(String),
    Run(String),
    Result(String),
    RecordedResult(String, Uuid),
    RunStatus {
        run: String,
        reconcile: bool,
        cancel: bool,
    },
    Preview(std::path::PathBuf),
    Proposal(AutomationDefinitionV1),
    Create(AutomationDraft),
    Update(String, AutomationDraft),
    DeletePreview(String),
    DeleteConfirmed(AutomationDeleteDraft),
}

#[derive(Debug)]
pub struct AutomationUiOutput {
    body: String,
    draft: Option<AutomationDraft>,
    delete_draft: Option<AutomationDeleteDraft>,
    session: Option<Uuid>,
    navigate: bool,
}

#[derive(Debug)]
pub struct AutomationUiResult {
    pub id: Uuid,
    pub result: Result<AutomationUiOutput, String>,
}

impl AutomationUiState {
    pub fn request_proposal(&mut self, definition: AutomationDefinitionV1) -> Result<(), String> {
        if self.in_flight >= 4 {
            return Err("Four automation operations are already pending".into());
        }
        let id = Uuid::new_v4();
        self.latest_preview = Some(id);
        self.draft = None;
        self.delete_draft = None;
        self.in_flight += 1;
        self.pending.push_back(AutomationUiRequest {
            id,
            action: AutomationUiAction::Proposal(definition),
        });
        Ok(())
    }
    pub fn request(&mut self, args: &str, cwd: &Path) -> Result<(), String> {
        if self.in_flight >= 4 {
            return Err("Four automation operations are already pending".into());
        }
        let tokens = if args.trim().is_empty() {
            Vec::new()
        } else {
            crate::permission::parse_simple_command(args).map_err(|_| USAGE.to_string())?
        };
        let fields = tokens.iter().map(String::as_str).collect::<Vec<_>>();
        let id = Uuid::new_v4();
        let action = match fields.as_slice() {
            [] | ["list"] => AutomationUiAction::List,
            ["inspect", id] => AutomationUiAction::Inspect((*id).into()),
            ["history"] => AutomationUiAction::History(None),
            ["history", id] => AutomationUiAction::History(Some((*id).into())),
            ["pause", id] => AutomationUiAction::Pause((*id).into()),
            ["resume", id] => AutomationUiAction::Resume((*id).into()),
            ["run", id] => AutomationUiAction::Run((*id).into()),
            ["status", run] => AutomationUiAction::RunStatus {
                run: (*run).into(),
                reconcile: false,
                cancel: false,
            },
            ["cancel", run] => AutomationUiAction::RunStatus {
                run: (*run).into(),
                reconcile: false,
                cancel: true,
            },
            ["reconcile", run] => AutomationUiAction::RunStatus {
                run: (*run).into(),
                reconcile: true,
                cancel: false,
            },
            ["result", id] => AutomationUiAction::Result((*id).into()),
            ["result", id, run] => AutomationUiAction::RecordedResult(
                (*id).into(),
                Uuid::parse_str(run).map_err(|_| "Result run must be a full UUID".to_string())?,
            ),
            ["preview", path] => {
                self.latest_preview = Some(id);
                self.draft = None;
                self.delete_draft = None;
                AutomationUiAction::Preview(std::path::PathBuf::from(
                    crate::agent::tools::resolve_path(path, &cwd.to_string_lossy()),
                ))
            }
            ["create", token] => AutomationUiAction::Create(self.confirmed_draft(token, None)?),
            ["update", target, token] => AutomationUiAction::Update(
                (*target).into(),
                self.confirmed_draft(token, Some(target))?,
            ),
            ["delete", target] => {
                self.latest_preview = Some(id);
                self.draft = None;
                self.delete_draft = None;
                AutomationUiAction::DeletePreview((*target).into())
            }
            ["delete", target, token] => {
                AutomationUiAction::DeleteConfirmed(self.confirmed_delete(target, token)?)
            }
            _ => return Err(USAGE.into()),
        };
        self.in_flight += 1;
        self.pending.push_back(AutomationUiRequest { id, action });
        Ok(())
    }

    fn confirmed_draft(
        &mut self,
        token: &str,
        target: Option<&str>,
    ) -> Result<AutomationDraft, String> {
        let draft = self
            .draft
            .as_ref()
            .ok_or("Preview the exact definition with /automation preview FILE first")?;
        if token != draft.digest || target.is_some_and(|id| id != draft.definition.id) {
            return Err("Confirmation does not match the current preview definition".into());
        }
        Ok(self.draft.take().expect("draft checked"))
    }

    fn confirmed_delete(&mut self, id: &str, token: &str) -> Result<AutomationDeleteDraft, String> {
        let draft = self
            .delete_draft
            .as_ref()
            .ok_or("Preview deletion with /automation delete ID first")?;
        if id != draft.id || token != draft.digest {
            return Err(
                "Deletion confirmation does not match the exact reviewed automation state".into(),
            );
        }
        Ok(self.delete_draft.take().expect("delete draft checked"))
    }
}

pub async fn execute(
    request: AutomationUiRequest,
    paths: RuntimePaths,
    profile: ActiveProfile,
) -> AutomationUiResult {
    let id = request.id;
    let result = if let AutomationUiAction::Run(automation_id) = request.action {
        crate::automation::run_now(&paths, &profile, &automation_id)
            .await
            .map(|session| AutomationUiOutput {
                body: format!(
                    "Automation {automation_id} finished{}",
                    session.map_or(
                        " quietly · private run result recorded".into(),
                        |id| format!(
                            "\nResult session: {id}\nOpen with /automation result {automation_id}"
                        )
                    )
                ),
                draft: None,
                delete_draft: None,
                session,
                navigate: false,
            })
            .map_err(|error| error.to_string())
    } else {
        tokio::task::spawn_blocking(move || execute_sync(request.action, &paths, &profile))
            .await
            .map_err(|_| "Automation management worker failed".to_string())
            .and_then(|result| result)
    };
    AutomationUiResult { id, result }
}

fn execute_sync(
    action: AutomationUiAction,
    paths: &RuntimePaths,
    profile: &ActiveProfile,
) -> Result<AutomationUiOutput, String> {
    if matches!(profile, ActiveProfile::Bare) {
        return Err("Automations require a persistent profile".into());
    }
    let store = AutomationStore::for_paths(paths, profile);
    let now = chrono::Utc::now();
    let output = |body| AutomationUiOutput {
        body,
        draft: None,
        delete_draft: None,
        session: None,
        navigate: false,
    };
    let find = |id: &str| {
        store
            .records()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|record| record.definition.id == id)
            .ok_or_else(|| format!("Automation {id} was not found"))
    };
    match action {
        AutomationUiAction::List => {
            let records = store.records().map_err(|error| error.to_string())?;
            let mut lines = vec![
                format!("Local automations · {}\n", profile.label()),
                USAGE.into(),
            ];
            for record in records.into_iter().take(128) {
                lines.push(format!(
                    "\n{} · {} · {} · next={}\nroute={}/{} · notifications={:?}{}",
                    record.definition.id,
                    crate::dashboard_snapshot::bounded_text(&record.definition.name, 128),
                    if record.blocked_reason.is_some() {
                        "needs review"
                    } else if record.enabled {
                        "active"
                    } else {
                        "paused"
                    },
                    record
                        .next_occurrence
                        .as_ref()
                        .map_or("none".into(), |occurrence| occurrence.at.to_rfc3339()),
                    record
                        .definition
                        .schedule
                        .provider
                        .as_deref()
                        .unwrap_or("none"),
                    record
                        .definition
                        .schedule
                        .model
                        .as_deref()
                        .unwrap_or("none"),
                    record.definition.notification_policy,
                    record
                        .active_run_id
                        .map_or(String::new(), |id| format!("\nCurrent run: {id}"))
                ));
            }
            Ok(output(lines.join("\n")))
        }
        AutomationUiAction::Inspect(id) => {
            let record = find(&id)?;
            Ok(output(
                serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?,
            ))
        }
        AutomationUiAction::RunStatus {
            run,
            reconcile,
            cancel,
        } => {
            let receipt =
                crate::automation_runtime::inspect_run(paths, profile, &run, reconcile, cancel)?;
            if reconcile {
                crate::automation::recover_pending_deliveries(paths, profile)
                    .map_err(|e| e.to_string())?;
            }
            Ok(output(
                serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?,
            ))
        }
        AutomationUiAction::History(id) => {
            let events = crate::automation::run_history(paths, profile, id.as_deref())
                .map_err(|error| error.to_string())?;
            let mut lines = vec!["Automation history · latest 64 receipts".into()];
            for event in events.iter().rev().take(64) {
                lines.push(format!(
                    "{} · {} · run {} · {:?}{}",
                    event.at,
                    event.automation_id,
                    event.run_id,
                    event.state,
                    event.saved_session_id.map_or_else(
                        || {
                            if matches!(
                                event.state,
                                crate::automation::AutomationRunStateV1::Succeeded
                            ) {
                                " · completed quietly · private result".into()
                            } else {
                                String::new()
                            }
                        },
                        |id| format!(" · result {id}")
                    )
                ));
            }
            lines.push("\nInspect: /automation status RUN_UUID · cancel RUN_UUID · reconcile RUN_UUID\nPrivate result: /automation result ID RUN_UUID".into());
            Ok(output(lines.join("\n")))
        }
        AutomationUiAction::Pause(id) => {
            store
                .set_enabled(&id, false, now)
                .map_err(|error| error.to_string())?;
            Ok(output(format!("Automation {id} paused")))
        }
        AutomationUiAction::Resume(id) => {
            store
                .set_enabled(&id, true, now)
                .map_err(|error| error.to_string())?;
            Ok(output(format!("Automation {id} resumed")))
        }
        AutomationUiAction::Result(id) => {
            let events = crate::automation::run_history(paths, profile, Some(&id))
                .map_err(|error| error.to_string())?;
            let session = events
                .iter()
                .rev()
                .find_map(|event| event.saved_session_id)
                .ok_or("No saved result session is available")?;
            Ok(AutomationUiOutput {
                body: format!("Automation {id} result session: {session}"),
                draft: None,
                delete_draft: None,
                session: Some(session),
                navigate: true,
            })
        }
        AutomationUiAction::RecordedResult(id, run) => {
            let body = crate::automation::read_recorded_result(paths, profile, &id, run)
                .map_err(|error| error.to_string())?;
            Ok(output(format!(
                "Recorded automation result · {id} · run {run}\n\n{body}"
            )))
        }
        AutomationUiAction::DeletePreview(id) => {
            let record = find(&id)?;
            let digest = crate::automation::automation_record_digest(&record)
                .map_err(|error| error.to_string())?;
            let json = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
            if json.len() > 128 * 1024 {
                return Err("Automation state exceeds the 128KiB deletion preview limit".into());
            }
            Ok(AutomationUiOutput {
                body: format!("Automation deletion preview\n\n{json}\n\nDelete definition {id}; immutable run receipts and results remain available. A live run blocks deletion.\nExact deletion confirmation: {digest}\nDelete: /automation delete {id} {digest}"),
                draft: None, delete_draft: Some(AutomationDeleteDraft { id, digest }),
                session: None, navigate: false,
            })
        }
        AutomationUiAction::DeleteConfirmed(draft) => {
            store
                .remove_confirmed_snapshot(&draft.id, &draft.digest)
                .map_err(|error| error.to_string())?;
            Ok(output(format!(
                "Automation {} deleted; immutable run receipts retained",
                draft.id
            )))
        }
        AutomationUiAction::Preview(path) => {
            let definition = crate::automation::prepare_definition(paths, profile, &path, now)
                .map_err(|error| error.to_string())?;
            preview_definition(definition)
        }
        AutomationUiAction::Proposal(definition) => {
            let definition =
                crate::automation::prepare_proposed_definition(paths, profile, definition, now)
                    .map_err(|error| error.to_string())?;
            preview_definition(definition)
        }
        AutomationUiAction::Create(draft) => {
            crate::automation::validate_configured_definition(
                paths,
                profile,
                &draft.definition,
                now,
            )
            .map_err(|error| error.to_string())?;
            let classic_count = crate::schedule::configured_schedule_count(paths, profile)
                .map_err(|error| error.to_string())?;
            let record = store
                .add_confirmed(draft.definition, &draft.digest, classic_count, now)
                .map_err(|error| error.to_string())?;
            Ok(output(format!(
                "Automation {} created and enabled",
                record.definition.id
            )))
        }
        AutomationUiAction::Update(id, draft) => {
            crate::automation::validate_configured_definition(
                paths,
                profile,
                &draft.definition,
                now,
            )
            .map_err(|error| error.to_string())?;
            store
                .update_confirmed(&id, draft.definition, &draft.digest, now)
                .map_err(|error| error.to_string())?;
            Ok(output(format!("Automation {id} updated")))
        }
        AutomationUiAction::Run(_) => unreachable!("async run handled separately"),
    }
}

fn preview_definition(definition: AutomationDefinitionV1) -> Result<AutomationUiOutput, String> {
    let digest =
        crate::automation::definition_digest(&definition).map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&definition).map_err(|error| error.to_string())?;
    if json.len() > 128 * 1024 {
        return Err("Definition exceeds the 128KiB preview limit".into());
    }
    let body = format!("Automation definition preview\n\n{json}\n\nExact confirmation: {digest}\nCreate: /automation create {digest}\nUpdate existing {}: /automation update {} {digest}\n\nThe preview stages a definition; it does not enable or change an automation.", definition.id, definition.id);
    Ok(AutomationUiOutput {
        body,
        draft: Some(AutomationDraft { definition, digest }),
        delete_draft: None,
        session: None,
        navigate: false,
    })
}

pub fn apply(state: &mut crate::app::AppState, result: AutomationUiResult) {
    state.automation_ui.in_flight = state.automation_ui.in_flight.saturating_sub(1);
    match result.result {
        Ok(output) => {
            if (output.draft.is_some() || output.delete_draft.is_some())
                && state.automation_ui.latest_preview != Some(result.id)
            {
                return;
            }
            if let Some(draft) = output.draft {
                state.automation_ui.draft = Some(draft);
            }
            if let Some(draft) = output.delete_draft {
                state.automation_ui.delete_draft = Some(draft);
            }
            state.status_message = Some(
                output
                    .body
                    .lines()
                    .next()
                    .unwrap_or("Automation operation completed")
                    .into(),
            );
            if output.navigate {
                state.pending_input_action = output
                    .session
                    .map(|id| crate::tui::commands::InputAction::LoadSession(id.to_string()));
            } else {
                state.plan_reader = None;
                state.learning_review_pager_open = false;
                state.pager_content = crate::terminal_text::sanitize_terminal_text(&output.body);
                state.pager_scroll = 0;
                state.pager_selection.latest();
                state.pager_open = true;
            }
        }
        Err(error) => {
            state.status_message = Some(format!(
                "Automation operation failed: {}",
                crate::dashboard_snapshot::bounded_text(&error, 1024)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_definition(workspace: &Path) -> AutomationDefinitionV1 {
        serde_json::from_value(serde_json::json!({
            "schema": crate::automation::AUTOMATION_SCHEMA,
            "id":"fixture-review", "name":"Reviewed original",
            "automation_kind":{"kind":"standalone"},
            "schedule":{
                "version":2,"id":"fixture-review","provider":"local","model":"fixture",
                "workspace":workspace,"runtime_secs":30,"output_bytes":1024,
                "tools":[],"grants":[],"allow_network":false,"skills":[],
                "workspace_policy":"existing_read_only","pinned_revision":null,
                "trigger":{"kind":"interval","seconds":3600,"anchor":null},
                "missed_run_policy":"skip","target":{"kind":"prompt","prompt":"Fixture only"},
                "delivery":{"kind":"none"}
            },
            "reasoning_effort":null,"pricing":{"kind":"local_zero"},"context_sha256":[],
            "notification_policy":"all_results"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn preview_confirmation_freezes_definition_and_rechecks_current_authority() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        let config = "[providers.local]\nbase_url='http://127.0.0.1:1/v1'\napi_key=''\nmodels=['fixture']\n[defaults]\nprovider='local'\nmodel='fixture'\n";
        std::fs::write(&paths.config_file, config).unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let source = temp.path().join("draft.json");
        let definition = fixture_definition(&workspace);
        let work = std::sync::Arc::new(std::sync::Mutex::new(crate::work::WorkState::default()));
        assert!(crate::work::execute_model_tool(
            "propose_automation",
            &serde_json::json!({"definition":definition}),
            &work
        )
        .unwrap()
        .is_ok());
        {
            let locked = work.lock().unwrap();
            assert!(locked.pending_automation_proposal.is_some());
            assert!(serde_json::to_value(&*locked)
                .unwrap()
                .get("pending_automation_proposal")
                .is_none());
            assert!(locked.fork_full().pending_automation_proposal.is_none());
        }
        assert!(crate::work::execute_model_tool(
            "propose_automation",
            &serde_json::json!({"definition":definition}),
            &work
        )
        .unwrap()
        .is_err());
        let proposal = work
            .lock()
            .unwrap()
            .pending_automation_proposal
            .take()
            .unwrap();
        let mut conversational_ui = AutomationUiState::default();
        conversational_ui.request_proposal(proposal).unwrap();
        let preview = execute(
            conversational_ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default,
        )
        .await
        .result
        .unwrap();
        assert!(preview.draft.is_some());
        assert!(AutomationStore::for_paths(&paths, &ActiveProfile::Default)
            .records()
            .unwrap()
            .is_empty());
        assert!(crate::work::execute_model_tool(
            "propose_automation",
            &serde_json::json!({"definition":{},"extra":"ignored"}),
            &work
        )
        .unwrap()
        .is_err());
        std::fs::write(&source, serde_json::to_vec(&definition).unwrap()).unwrap();
        let mut ui = AutomationUiState::default();
        ui.request(&format!("preview {}", source.display()), &workspace)
            .unwrap();
        let preview = execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default,
        )
        .await
        .result
        .unwrap();
        assert!(AutomationStore::for_paths(&paths, &ActiveProfile::Default)
            .records()
            .unwrap()
            .is_empty());
        let draft = preview.draft.unwrap();
        let digest = draft.digest.clone();
        ui.in_flight = 0;
        ui.draft = Some(draft);
        assert!(ui.request("create wrong", &workspace).is_err());
        assert!(ui
            .request(&format!("update other-id {digest}"), &workspace)
            .is_err());
        assert!(ui.draft.is_some());
        let mut changed = definition.clone();
        changed.name = "Unreviewed source edit".into();
        std::fs::write(&source, serde_json::to_vec(&changed).unwrap()).unwrap();
        ui.request(&format!("create {digest}"), &workspace).unwrap();
        assert!(ui.draft.is_none());
        assert!(execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default
        )
        .await
        .result
        .is_ok());
        let records = AutomationStore::for_paths(&paths, &ActiveProfile::Default)
            .records()
            .unwrap();
        assert_eq!(records[0].definition.name, "Reviewed original");
        assert!(ui.request(&format!("create {digest}"), &workspace).is_err());
        ui.in_flight = 0;
        ui.draft = Some(AutomationDraft {
            definition: records[0].definition.clone(),
            digest: records[0].definition_sha256.clone(),
        });
        ui.request(&format!("update fixture-review {digest}"), &workspace)
            .unwrap();
        std::fs::write(
            &paths.config_file,
            config.replace("127.0.0.1", "remote.invalid"),
        )
        .unwrap();
        assert!(execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default
        )
        .await
        .result
        .is_err());
        assert_eq!(
            AutomationStore::for_paths(&paths, &ActiveProfile::Default)
                .records()
                .unwrap()[0]
                .definition
                .name,
            "Reviewed original"
        );
    }

    #[test]
    fn a_new_preview_invalidates_an_older_confirmation_immediately() {
        let mut ui = AutomationUiState {
            draft: Some(AutomationDraft {
                definition: fixture_definition(Path::new("/tmp")),
                digest: "old-token".into(),
            }),
            ..Default::default()
        };
        ui.request("preview newer.json", Path::new("/tmp")).unwrap();
        assert!(ui.request("create old-token", Path::new("/tmp")).is_err());
    }
    #[test]
    fn commands_never_create_without_exact_current_preview_confirmation() {
        let mut ui = AutomationUiState::default();
        assert!(ui.request("create arbitrary", Path::new("/tmp")).is_err());
        assert!(ui
            .request("update target arbitrary", Path::new("/tmp"))
            .is_err());
        assert!(ui.pending.is_empty());
        ui.request("preview 'draft with spaces.json'", Path::new("/tmp"))
            .unwrap();
        assert!(
            matches!(ui.pending[0].action, AutomationUiAction::Preview(ref path) if path == Path::new("/tmp/draft with spaces.json"))
        );
        assert!(ui.request("create", Path::new("/tmp")).is_err());
    }

    #[tokio::test]
    async fn deletion_confirmation_is_once_and_rejects_state_drift() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        std::fs::write(&paths.config_file, "[providers.local]\nbase_url='http://127.0.0.1:1/v1'\napi_key=''\nmodels=['fixture']\n[defaults]\nprovider='local'\nmodel='fixture'\n").unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = AutomationStore::for_paths(&paths, &ActiveProfile::Default);
        let definition = crate::automation::prepare_proposed_definition(
            &paths,
            &ActiveProfile::Default,
            fixture_definition(&workspace),
            chrono::Utc::now(),
        )
        .unwrap();
        let digest = crate::automation::definition_digest(&definition).unwrap();
        store
            .add_confirmed(definition, &digest, 0, chrono::Utc::now())
            .unwrap();
        let mut ui = AutomationUiState::default();
        assert!(ui
            .request("delete fixture-review arbitrary", &workspace)
            .is_err());
        ui.request("delete fixture-review", &workspace).unwrap();
        let preview = execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default,
        )
        .await
        .result
        .unwrap();
        ui.in_flight = 0;
        ui.delete_draft = preview.delete_draft;
        let token = ui.delete_draft.as_ref().unwrap().digest.clone();
        assert!(ui
            .request("delete fixture-review wrong", &workspace)
            .is_err());
        assert!(ui.delete_draft.is_some());
        store
            .set_enabled("fixture-review", false, chrono::Utc::now())
            .unwrap();
        ui.request(&format!("delete fixture-review {token}"), &workspace)
            .unwrap();
        assert!(execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default
        )
        .await
        .result
        .is_err());
        assert_eq!(store.records().unwrap().len(), 1);
        ui.in_flight = 0;
        assert!(ui
            .request(&format!("delete fixture-review {token}"), &workspace)
            .is_err());
        ui.request("delete fixture-review", &workspace).unwrap();
        let preview = execute(
            ui.pending.pop_front().unwrap(),
            paths.clone(),
            ActiveProfile::Default,
        )
        .await
        .result
        .unwrap();
        ui.in_flight = 0;
        ui.delete_draft = preview.delete_draft;
        let token = ui.delete_draft.as_ref().unwrap().digest.clone();
        ui.request(&format!("delete fixture-review {token}"), &workspace)
            .unwrap();
        assert!(execute(
            ui.pending.pop_front().unwrap(),
            paths,
            ActiveProfile::Default
        )
        .await
        .result
        .is_ok());
        assert!(store.records().unwrap().is_empty());
        ui.in_flight = 0;
        assert!(ui
            .request(&format!("delete fixture-review {token}"), &workspace)
            .is_err());
    }

    #[tokio::test]
    async fn model_metadata_is_bounded_profile_scoped_and_excludes_private_definition_fields() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        std::fs::write(&paths.config_file, "[providers.local]\nbase_url='http://127.0.0.1:1/v1'\napi_key=''\nmodels=['fixture']\n[defaults]\nprovider='local'\nmodel='fixture'\n").unwrap();
        let workspace = temp.path().join("private-workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = AutomationStore::for_paths(&paths, &ActiveProfile::Default);
        for id in ["fixture-review", "fixture-a"] {
            let mut definition = fixture_definition(&workspace);
            definition.id = id.into();
            definition.schedule.id = id.into();
            definition.schedule.target = crate::schedule_v2::ScheduleTargetV2::Prompt {
                prompt: "ORC258_PRIVATE_DEFINITION_PROMPT".into(),
            };
            let definition = crate::automation::prepare_proposed_definition(
                &paths,
                &ActiveProfile::Default,
                definition,
                chrono::Utc::now(),
            )
            .unwrap();
            let digest = crate::automation::definition_digest(&definition).unwrap();
            store
                .add_confirmed(definition, &digest, 0, chrono::Utc::now())
                .unwrap();
        }
        store
            .set_enabled("fixture-a", false, chrono::Utc::now())
            .unwrap();
        let scope = AutomationToolScope {
            paths,
            profile: ActiveProfile::Default,
        };
        let result = list_model_metadata(scope.clone(), &serde_json::json!({"limit":1}), 5).await;
        assert!(result.is_success(), "{}", result.model_text());
        let text = result.model_text();
        assert!(text.len() <= 32 * 1024);
        assert!(!text.contains("ORC258_PRIVATE_DEFINITION_PROMPT"));
        assert!(!text.contains(&workspace.to_string_lossy().to_string()));
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["total"], 2);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["automations"][0]["id"], "fixture-a");
        assert_eq!(value["automations"][0]["state"], "paused");
        let keys = value["automations"][0].as_object().unwrap();
        assert_eq!(keys.len(), 8);
        let next =
            list_model_metadata(scope.clone(), &serde_json::json!({"offset":1,"limit":1}), 5).await;
        assert!(next.model_text().contains("fixture-review"));
        for arguments in [
            serde_json::json!({"limit":0}),
            serde_json::json!({"limit":26}),
            serde_json::json!({"offset":256}),
            serde_json::json!({"private":true}),
        ] {
            assert!(!list_model_metadata(scope.clone(), &arguments, 5)
                .await
                .is_success());
        }
        let bare = AutomationToolScope {
            profile: ActiveProfile::Bare,
            ..scope
        };
        assert!(!list_model_metadata(bare, &serde_json::json!({}), 5)
            .await
            .is_success());
    }
}
