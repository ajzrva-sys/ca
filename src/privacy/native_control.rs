//! Host-issued child control metadata. Arbitrary native output stays opaque.
use super::{digest, Context, ReadReceipt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_CONTROL_IDS: usize = 4096;
const MAX_SAFE_TEMPLATES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypedNativeControl {
    pub agents: Vec<NativeAgentControl>,
    pub batch: Option<NativeAgentControl>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeAgentControl {
    pub id: Uuid,
    pub status: NativeAgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrow_repair: Option<NativeNarrowRepairControl>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeNarrowRepairControl {
    pub target_path: String,
    pub contract_sha256: String,
    pub source_sha256: String,
    pub attempts: u8,
    pub checks: Vec<NativeRepairCheckControl>,
    pub usage: Option<NativeRepairUsageControl>,
    #[serde(default)]
    pub unresolved_conditions: Vec<String>,
    #[serde(skip)]
    host_issued: bool,
    #[serde(skip)]
    verified: bool,
    #[serde(skip)]
    applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeRepairCheckControl {
    pub command_sha256: String,
    pub exit_code: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeRepairUsageControl {
    /// Completed provider turns, not a physical retry or cost estimate.
    pub requests: u64,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

impl NativeNarrowRepairControl {
    pub(crate) fn issued(
        target_path: String,
        contract_sha256: String,
        source_sha256: String,
        attempts: u8,
        checks: Vec<NativeRepairCheckControl>,
        usage: Option<NativeRepairUsageControl>,
    ) -> Self {
        Self {
            target_path,
            contract_sha256,
            source_sha256,
            attempts,
            checks,
            usage,
            unresolved_conditions: Vec::new(),
            host_issued: true,
            verified: true,
            applied: false,
        }
    }

    pub(crate) fn issued_not_verified(
        target_path: String,
        contract_sha256: String,
        admitted_source_sha256: String,
        usage: Option<NativeRepairUsageControl>,
    ) -> Self {
        Self {
            target_path,
            contract_sha256,
            source_sha256: admitted_source_sha256,
            attempts: 0,
            checks: Vec::new(),
            usage,
            unresolved_conditions: vec!["not_verified".into()],
            host_issued: true,
            verified: false,
            applied: false,
        }
    }

    pub(crate) fn issued_applied(
        target_path: String,
        contract_sha256: String,
        applied_source_sha256: String,
        attempts: u8,
        usage: Option<NativeRepairUsageControl>,
    ) -> Self {
        Self {
            target_path,
            contract_sha256,
            source_sha256: applied_source_sha256,
            attempts,
            checks: Vec::new(),
            usage,
            unresolved_conditions: vec!["not_verified".into()],
            host_issued: true,
            verified: false,
            applied: true,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let hash = |text: &str| {
            text.len() == 64
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        };
        let path = std::path::Path::new(&self.target_path);
        let verified_shape = (1..=3).contains(&self.attempts)
            && !self.checks.is_empty()
            && self.unresolved_conditions.is_empty();
        let admitted_shape = self.attempts == 0
            && self.checks.is_empty()
            && self.unresolved_conditions == ["not_verified"];
        let applied_shape = (1..=3).contains(&self.attempts)
            && self.checks.is_empty()
            && self.unresolved_conditions == ["not_verified"];
        let shape = if self.host_issued {
            if self.verified {
                verified_shape
            } else if self.applied {
                applied_shape
            } else {
                admitted_shape
            }
        } else {
            verified_shape || admitted_shape || applied_shape
        };
        if !path.is_absolute()
            || self.target_path.len() > 4096
            || self.target_path.chars().any(char::is_control)
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
            || !hash(&self.contract_sha256)
            || !hash(&self.source_sha256)
            || !shape
            || self.checks.len() > 8
            || self
                .checks
                .iter()
                .any(|check| check.exit_code != 0 || !hash(&check.command_sha256))
            || (self.host_issued
                && self.verified
                && self.usage.as_ref().is_some_and(|usage| usage.requests == 0))
        {
            return Err("Invalid native narrow repair evidence".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeAgentStatus {
    Queued,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

impl TypedNativeControl {
    fn validate(&self) -> Result<(), String> {
        if self
            .agents
            .len()
            .saturating_add(usize::from(self.batch.is_some()))
            > MAX_CONTROL_IDS
        {
            return Err("Native control metadata exceeds its bounded ID count".into());
        }
        let mut ids = BTreeSet::new();
        for control in self.agents.iter().chain(self.batch.iter()) {
            if control.id.is_nil() || !ids.insert(control.id) {
                return Err("Native control metadata contains an invalid or duplicate ID".into());
            }
            if let Some(repair) = &control.narrow_repair {
                if repair.host_issued
                    && repair.verified
                    && control.status != NativeAgentStatus::Completed
                {
                    return Err("Unfinished agent cannot publish verified repair evidence".into());
                }
                repair.validate()?;
            }
        }
        if self
            .batch
            .as_ref()
            .is_some_and(|batch| batch.narrow_repair.is_some())
        {
            return Err("A batch cannot claim one agent's repair evidence".into());
        }
        Ok(())
    }
}

impl Context {
    /// Persist host-issued resource identity before publishing it or starting
    /// its external action. Ownership checks remain with the native manager.
    pub(crate) fn bind_native_id(&self, kind: &str, id: Uuid) -> Result<(), String> {
        if !matches!(kind, "JOB" | "WATCH") || id.is_nil() {
            return Err("Invalid native resource identity".into());
        }
        let value = id.to_string();
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let token = state.registry.alias(kind, &value)?;
            state.allowed.remove(&value);
            state.active.insert(value, token);
            state.matcher_dirty = true;
            self.private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        self.commit_registry()
    }

    pub(crate) fn record_native_control(
        &self,
        original: &str,
        control: TypedNativeControl,
    ) -> Result<String, String> {
        control.validate()?;
        if original.len() > super::MAX_SOURCE_BYTES {
            return Err("Native control result exceeds its private source bound".into());
        }
        let id = Uuid::new_v4().to_string();
        let receipt = ReadReceipt {
            id: Some(id.clone()),
            path: String::new(),
            source: String::new(),
            source_digest: digest(original.as_bytes()),
            original: original.to_string(),
            start: 0,
            end: 0,
            hashes: false,
            read_footer: None,
            attachment: None,
            native_control: Some(control.clone()),
        };
        let encoded = serde_json::to_string(&receipt)
            .map_err(|_| "Native control source cannot be encoded")?;
        {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            state.registry.alias("SOURCE_RECEIPT", &encoded)?;
            state.reads.insert(id.clone(), receipt);
        }
        // Allocate the same immutable IDs immediately, so authored follow-up
        // arguments and this source projection use one identity throughout.
        project(self, original, &control)?;
        Ok(id)
    }
}

pub(super) fn project(
    context: &Context,
    original: &str,
    control: &TypedNativeControl,
) -> Result<String, String> {
    control.validate()?;
    // Resolve only existing, currently authorized references. This does not
    // reread source bytes or upgrade the receipt's recorded-source freshness.
    let target_refs = control
        .agents
        .iter()
        .map(|agent| {
            agent
                .narrow_repair
                .as_ref()
                .filter(|repair| repair.host_issued)
                .and_then(|repair| {
                    let path = std::path::Path::new(&repair.target_path);
                    if !path.is_file() || std::fs::canonicalize(path).ok().as_deref() != Some(path)
                    {
                        return None;
                    }
                    if context.filenames_enabled() {
                        let reference = context.project_filename_path(path).ok()?;
                        (context
                            .resolve_filename_path(&reference, false)
                            .ok()
                            .as_deref()
                            == Some(path))
                        .then_some(reference)
                    } else {
                        let allowed = path.starts_with(context.workspace())
                            || context
                                .root_authority
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .iter()
                                .any(|root| path.starts_with(root));
                        allowed.then(|| repair.target_path.clone())
                    }
                })
        })
        .collect::<Vec<_>>();
    let fingerprint = context.policy_fingerprint();
    let mut state = context.state.lock().unwrap_or_else(|p| p.into_inner());
    let mut alias = |kind: &str, value: &str| -> Result<String, String> {
        if kind == "DIGEST" {
            // A field-local identity does not classify previously public text.
            // Its separate registry kind also survives policy/load cycles
            // without capturing later ordinary private-content identities.
            return state.registry.alias("NATIVE_DIGEST", value);
        }
        let token = state.registry.alias(kind, value)?;
        let mut changed = state.allowed.remove(value);
        changed |= state
            .active
            .insert(value.to_string(), token.clone())
            .as_ref()
            != Some(&token);
        if changed {
            // Keep partial local allocation failures fail-closed too: a later
            // alias or allowance limit must not leave the old matcher cached.
            state.matcher_dirty = true;
            context
                .private_revision
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        Ok(token)
    };
    let agents = control
        .agents
        .iter()
        .zip(target_refs)
        .map(|(control, target_ref)| {
            let evidence = match &control.narrow_repair {
                Some(repair) if repair.host_issued => "host_issued",
                Some(_) => "unavailable_after_restore",
                None => "none",
            };
            let repair = if let Some(repair) = control.narrow_repair.as_ref().filter(|repair| repair.host_issued) {
                let target_ref = target_ref.map(|reference| {
                    if context.filenames_enabled() { Ok(reference) } else { alias("PATH", &reference) }
                }).transpose()?;
                let checks = repair.checks.iter().map(|check| Ok(json!({"command_sha256":alias("DIGEST", &check.command_sha256)?,"exit_code":check.exit_code}))).collect::<Result<Vec<_>, String>>()?;
                let usage = repair.usage.as_ref().map(|usage| json!({"requests":usage.requests,"request_count_kind":"completed_provider_turns","prompt_tokens":usage.prompt_tokens,"completion_tokens":usage.completion_tokens}));
                Some(json!({"kind":"host_narrow_repair","target_reference":if target_ref.is_some(){"discovered_filename"}else{"unavailable"},"target_ref":target_ref,"verification":if repair.verified{"passed_for_recorded_source"}else{"not_verified"},"source_identity":if repair.verified{"verified_at_completion"}else if repair.applied{"applied_at_attempt"}else{"admitted"},"freshness":"not_revalidated","contract_sha256":alias("DIGEST", &repair.contract_sha256)?,"source_sha256":alias("DIGEST", &repair.source_sha256)?,"attempts":(repair.verified || repair.applied).then_some(repair.attempts),"checks":checks,"unresolved_conditions":repair.unresolved_conditions,"usage":usage}))
            } else { None };
            Ok(json!({"agent_id":alias("AGENT", &control.id.to_string())?,"status":control.status,"narrow_repair":repair,"narrow_repair_evidence":evidence}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let batch = if let Some(control) = &control.batch {
        Some(json!({"batch_id":alias("AGENT", &control.id.to_string())?,"status":control.status}))
    } else {
        None
    };
    let body = alias("TEXT", original)?;
    let text = format!(
        "Native tool metadata\n{}",
        json!({"agents":agents,"batch":batch,"body":body,"body_kind":"opaque_private_text","body_description":"Opaque private text; not a filename. Use typed metadata references for native operations."})
    );
    let key = (fingerprint, digest(text.as_bytes()));
    state
        .safe_template_hashes
        .retain(|_, ticket| ticket.valid());
    if let Some(ticket) = state.safe_template_hashes.get(&key) {
        ticket.touch();
    } else {
        if state.safe_template_hashes.len() >= MAX_SAFE_TEMPLATES {
            return Err(
                "Native control preview budget is full; prepare a shorter continuation".into(),
            );
        }
        let ticket = crate::cache_memory::reserve("privacy_native_controls", 256)
            .ok_or("Native control preview budget is unavailable; retry this request")?;
        // This map belongs to this exact registry/namespace. Its full policy
        // fingerprint and exact whole-string hash cannot authorize a leaf,
        // an altered copy, or an unrelated source's output.
        state.safe_template_hashes.insert(key, ticket);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::{policy::Policy, Context};
    use std::sync::Arc;

    fn fixture() -> (tempfile::TempDir, Arc<Context>) {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let context =
            Context::open(root.path().join("profile"), workspace, Uuid::new_v4(), None).unwrap();
        context.allow_ephemeral();
        context
            .save_policy(Policy {
                enabled: true,
                ..context.policy()
            })
            .unwrap();
        (root, context)
    }

    fn control(id: Uuid, status: NativeAgentStatus) -> TypedNativeControl {
        TypedNativeControl {
            agents: vec![NativeAgentControl {
                id,
                status,
                narrow_repair: None,
            }],
            batch: None,
        }
    }

    fn metadata(projected: &str) -> serde_json::Value {
        serde_json::from_str(projected.strip_prefix("Native tool metadata\n").unwrap()).unwrap()
    }

    #[test]
    fn privacy_native_control_retains_typed_ids_and_status_without_disclosing_body() {
        let (_root, context) = fixture();
        let id = Uuid::new_v4();
        let original = format!("agent {id} running private-task-canary /private/workspace-canary");
        let source = context
            .record_native_control(&original, control(id, NativeAgentStatus::Running))
            .unwrap();
        let projected = context.project_retrieved(&original, Some(&source)).unwrap();
        let data = metadata(&projected);
        assert_eq!(data["agents"][0]["status"], "running");
        let token = data["agents"][0]["agent_id"].as_str().unwrap();
        assert!(token.contains("_AGENT_"));
        assert_eq!(context.restore_text(token).unwrap(), id.to_string());
        assert_eq!(context.mask_known(&id.to_string()).unwrap(), token);
        for private in [
            id.to_string(),
            "private-task-canary".into(),
            "/private/workspace-canary".into(),
        ] {
            assert!(!projected.contains(&private));
        }
        assert_eq!(
            context
                .restore_text(data["body"].as_str().unwrap())
                .unwrap(),
            original
        );
        assert!(context.observed_sources().is_empty());
        assert!(context
            .project_retrieved("changed body", Some(&source))
            .is_err());
    }

    #[test]
    fn privacy_native_repair_receipt_exposes_only_host_evidence_and_usable_target_reference() {
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            structural: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
            let (_root, context) = fixture();
            let path = context.workspace().join("PrivateRepairTarget317.py");
            std::fs::write(&path, "VALUE = 1\n").unwrap();
            for name in [
                "completed",
                "passed_for_recorded_source",
                "not_revalidated",
                "0",
                "1",
            ] {
                std::fs::write(context.workspace().join(name), "private collision").unwrap();
            }
            context.refresh_filenames().unwrap();
            let path = path.canonicalize().unwrap();
            let repair = NativeNarrowRepairControl::issued(
                path.to_str().unwrap().into(),
                "a".repeat(64),
                "b".repeat(64),
                1,
                vec![NativeRepairCheckControl {
                    command_sha256: "c".repeat(64),
                    exit_code: 0,
                }],
                Some(NativeRepairUsageControl {
                    requests: 1,
                    prompt_tokens: Some(20),
                    completion_tokens: Some(4),
                }),
            );
            let id = Uuid::new_v4();
            let control = TypedNativeControl {
                agents: vec![NativeAgentControl {
                    id,
                    status: NativeAgentStatus::Completed,
                    narrow_repair: Some(repair.clone()),
                }],
                batch: None,
            };
            let original = format!(
                "PRIVATE_CHILD_TASK_CANARY PRIVATE_CHILD_OUTPUT_CANARY {}",
                json!({"schema":"ca.narrow-repair.v1","status":"complete","source_sha256":"d".repeat(64)})
            );
            let source = context
                .record_native_control(&original, control.clone())
                .unwrap();
            let projected = context.project_retrieved(&original, Some(&source)).unwrap();
            let data = metadata(&projected);
            let receipt = &data["agents"][0]["narrow_repair"];
            assert_eq!(data["body_kind"], "opaque_private_text");
            assert!(data["body_description"]
                .as_str()
                .unwrap()
                .contains("not a filename"));
            assert_eq!(receipt["verification"], "passed_for_recorded_source");
            assert_eq!(receipt["freshness"], "not_revalidated");
            assert_eq!(receipt["attempts"], 1);
            assert_eq!(receipt["checks"][0]["exit_code"], 0);
            assert_eq!(receipt["usage"]["requests"], 1);
            assert_eq!(
                receipt["usage"]["request_count_kind"],
                "completed_provider_turns"
            );
            assert_eq!(receipt["source_identity"], "verified_at_completion");
            assert_eq!(receipt["unresolved_conditions"], json!([]));
            let target_ref = receipt["target_ref"].as_str().unwrap();
            let read = context
                .prepare_edit_ticket("read_file", &json!({"path":target_ref}))
                .unwrap();
            assert_eq!(read["path"], path.to_str().unwrap());
            assert!(context
                .prepare_edit_ticket("read_file", &json!({"path":data["body"]}))
                .is_err());
            for status in [
                NativeAgentStatus::Failed,
                NativeAgentStatus::Cancelled,
                NativeAgentStatus::TimedOut,
            ] {
                let pending = TypedNativeControl {
                    agents: vec![NativeAgentControl {
                        id: Uuid::new_v4(),
                        status,
                        narrow_repair: Some(NativeNarrowRepairControl::issued_not_verified(
                            path.to_str().unwrap().into(),
                            "a".repeat(64),
                            "e".repeat(64),
                            Some(NativeRepairUsageControl {
                                requests: 0,
                                prompt_tokens: None,
                                completion_tokens: None,
                            }),
                        )),
                    }],
                    batch: None,
                };
                let pending_source = context.record_native_control(&original, pending).unwrap();
                let pending = context
                    .project_retrieved(&original, Some(&pending_source))
                    .unwrap();
                let data = metadata(&pending);
                let repair = &data["agents"][0]["narrow_repair"];
                assert_eq!(
                    data["agents"][0]["status"],
                    serde_json::to_value(status).unwrap()
                );
                assert_eq!(repair["verification"], "not_verified");
                assert_eq!(repair["source_identity"], "admitted");
                assert_eq!(repair["freshness"], "not_revalidated");
                assert_eq!(repair["unresolved_conditions"], json!(["not_verified"]));
                assert!(repair["attempts"].is_null());
                assert_eq!(repair["usage"]["requests"], 0);
                context
                    .validate_wire(
                        &serde_json::to_vec(
                            &json!({"messages":[{"role":"tool","content":pending}]}),
                        )
                        .unwrap(),
                        context.policy().revision,
                        &context.policy_fingerprint(),
                    )
                    .unwrap();
            }
            for private in [
                "PRIVATE_CHILD_TASK_CANARY",
                "PRIVATE_CHILD_OUTPUT_CANARY",
                path.to_str().unwrap(),
                "PrivateRepairTarget317.py",
                &"a".repeat(64),
                &"b".repeat(64),
                &"c".repeat(64),
                &"d".repeat(64),
            ] {
                assert!(
                    !projected.contains(private),
                    "private output or identity leaked: {private}"
                );
            }
            let applied = TypedNativeControl {
                agents: vec![NativeAgentControl {
                    id: Uuid::new_v4(),
                    status: NativeAgentStatus::Failed,
                    narrow_repair: Some(NativeNarrowRepairControl::issued_applied(
                        path.to_str().unwrap().into(),
                        "a".repeat(64),
                        "f".repeat(64),
                        2,
                        None,
                    )),
                }],
                batch: None,
            };
            let applied_source = context
                .record_native_control(&original, applied.clone())
                .unwrap();
            let applied_data = metadata(
                &context
                    .project_retrieved(&original, Some(&applied_source))
                    .unwrap(),
            );
            let applied_receipt = &applied_data["agents"][0]["narrow_repair"];
            assert_eq!(applied_receipt["verification"], "not_verified");
            assert_eq!(applied_receipt["source_identity"], "applied_at_attempt");
            assert_eq!(applied_receipt["attempts"], 2);
            assert_eq!(applied_receipt["freshness"], "not_revalidated");
            assert_eq!(
                applied_receipt["unresolved_conditions"],
                json!(["not_verified"])
            );
            assert_eq!(applied_receipt["checks"], json!([]));
            let applied_restored =
                serde_json::from_value(serde_json::to_value(&applied).unwrap()).unwrap();
            let applied_restored = context
                .record_native_control(&original, applied_restored)
                .unwrap();
            assert!(metadata(
                &context
                    .project_retrieved(&original, Some(&applied_restored))
                    .unwrap()
            )["agents"][0]["narrow_repair"]
                .is_null());
            assert_eq!(
                context
                    .restore_text(receipt["source_sha256"].as_str().unwrap())
                    .unwrap(),
                "b".repeat(64)
            );
            let canonical = vec![
                ChatMessage {
                    role: "assistant".into(),
                    content: crate::image_content::ModelContent::text("Await the worker"),
                    tool_calls: Some(vec![ToolCallMsg {
                        id: "await-worker".into(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "await_agent".into(),
                            arguments:
                                json!({"agent_id":data["agents"][0]["agent_id"],"timeout_secs":0})
                                    .to_string(),
                            arguments_retrieved_spans: vec![],
                        },
                    }]),
                    tool_call_id: None,
                    response_items: vec![],
                },
                ChatMessage {
                    role: "tool".into(),
                    content: crate::image_content::ModelContent::text(original.clone())
                        .with_source_receipt(Some(source.clone())),
                    tool_calls: None,
                    tool_call_id: Some("await-worker".into()),
                    response_items: vec![],
                },
            ];
            for _ in 0..2 {
                let mut request = crate::request::PreparedModelRequest::new(
                    crate::request::RequestKind::ToolContinuation,
                    "fixture",
                    "model",
                    canonical.clone(),
                    vec![],
                    Default::default(),
                );
                let revision = context.prepare_request(&mut request).unwrap();
                let sent = request.messages[1].content.as_str().unwrap();
                assert_eq!(
                    metadata(sent),
                    data,
                    "retry must retain the exact recorded receipt"
                );
                context
                    .validate_wire(
                        &serde_json::to_vec(&json!({"messages":request.messages})).unwrap(),
                        revision,
                        &context.policy_fingerprint(),
                    )
                    .unwrap();
            }

            let spoof_source = context
                .record_native_control(
                    &original,
                    super::tests::control(Uuid::new_v4(), NativeAgentStatus::Completed),
                )
                .unwrap();
            let spoof = metadata(
                &context
                    .project_retrieved(&original, Some(&spoof_source))
                    .unwrap(),
            );
            assert!(
                spoof["agents"][0]["narrow_repair"].is_null(),
                "receipt-looking model text grants no host evidence"
            );
            let restored: TypedNativeControl =
                serde_json::from_value(serde_json::to_value(&control).unwrap()).unwrap();
            let restored_source = context.record_native_control(&original, restored).unwrap();
            let restored = metadata(
                &context
                    .project_retrieved(&original, Some(&restored_source))
                    .unwrap(),
            );
            assert!(restored["agents"][0]["narrow_repair"].is_null());
            assert_eq!(
                restored["agents"][0]["narrow_repair_evidence"],
                "unavailable_after_restore"
            );
            let old: NativeAgentControl =
                serde_json::from_value(json!({"id":Uuid::new_v4(),"status":"completed"})).unwrap();
            assert!(old.narrow_repair.is_none());
            let mut forged = serde_json::to_value(&repair).unwrap();
            forged["host_issued"] = json!(true);
            assert!(serde_json::from_value::<NativeNarrowRepairControl>(forged).is_err());
            std::fs::write(&path, "VALUE = 2\n").unwrap();
            assert_eq!(
                metadata(&context.project_retrieved(&original, Some(&source)).unwrap())["agents"]
                    [0]["narrow_repair"]["freshness"],
                "not_revalidated"
            );
            std::fs::remove_file(&path).unwrap();
            assert!(
                metadata(&context.project_retrieved(&original, Some(&source)).unwrap())["agents"]
                    [0]["narrow_repair"]["target_ref"]
                    .is_null()
            );
        });
    }

    #[test]
    fn privacy_native_digest_fields_do_not_reclassify_existing_public_signed_prefix() {
        let settings = crate::provider_filter::ProviderFilterConfig {
            enabled: true,
            structural: true,
            hide: crate::provider_filter::FilterHide {
                filenames: true,
                ..Default::default()
            },
            ..Default::default()
        };
        crate::provider_filter::with_test_filter(&settings, || {
            use crate::agent::client::{ChatMessage, FunctionCall, ToolCallMsg};
            let (_root, context) = fixture();
            let source_text = "VALUE = 0\n";
            let target = context.workspace().join("PrivatePrefixTarget317.py");
            std::fs::write(&target, source_text).unwrap();
            let public_sha = crate::narrow_worker::digest(source_text);
            let private_sha = "f".repeat(64);
            std::fs::write(context.workspace().join(&private_sha), "private filename").unwrap();
            context.refresh_filenames().unwrap();
            let user = ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(format!(
                    "Repair source_sha256={public_sha}; private filename {private_sha}"
                )),
                tool_calls: None,
                tool_call_id: None,
                response_items: vec![],
            };
            let mut first = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::MainTurn,
                "fixture",
                "model",
                vec![user.clone()],
                vec![],
                Default::default(),
            );
            context.prepare_request(&mut first).unwrap();
            let first_text = first.messages[0].content.as_str().unwrap().to_owned();
            assert!(first_text.contains(&public_sha));
            assert!(!first_text.contains(&private_sha));
            let prefix = |text: &str| {
                crate::provider_replay::responses_prefix(&serde_json::to_vec(&json!({"model":"model","tools":[],"input":[{"role":"user","content":[{"type":"input_text","text":text}]}]})).unwrap(), Some(1)).unwrap()
            };
            let first_prefix = prefix(&first_text);
            let id = Uuid::new_v4();
            let original = "private native registration body";
            let source = context
                .record_native_control(
                    original,
                    TypedNativeControl {
                        agents: vec![NativeAgentControl {
                            id,
                            status: NativeAgentStatus::Running,
                            narrow_repair: Some(NativeNarrowRepairControl::issued_not_verified(
                                target.canonicalize().unwrap().to_str().unwrap().into(),
                                "a".repeat(64),
                                public_sha.clone(),
                                None,
                            )),
                        }],
                        batch: None,
                    },
                )
                .unwrap();
            let mut next = crate::request::PreparedModelRequest::new(
                crate::request::RequestKind::ToolContinuation,
                "fixture",
                "model",
                vec![
                    user,
                    ChatMessage {
                        role: "assistant".into(),
                        content: crate::image_content::ModelContent::text("Await repair"),
                        tool_calls: Some(vec![ToolCallMsg {
                            id: "await-prefix".into(),
                            call_type: "function".into(),
                            function: FunctionCall {
                                name: "await_agent".into(),
                                arguments: json!({"agent_id":id,"timeout_secs":0}).to_string(),
                                arguments_retrieved_spans: vec![],
                            },
                        }]),
                        tool_call_id: None,
                        response_items: vec![],
                    },
                    ChatMessage {
                        role: "tool".into(),
                        content: crate::image_content::ModelContent::text(original)
                            .with_source_receipt(Some(source)),
                        tool_calls: None,
                        tool_call_id: Some("await-prefix".into()),
                        response_items: vec![],
                    },
                ],
                vec![],
                Default::default(),
            );
            let revision = context.prepare_request(&mut next).unwrap();
            let next_text = next.messages[0].content.as_str().unwrap();
            assert_eq!(
                next_text, first_text,
                "native receipt fields cannot retroactively redact a public prefix"
            );
            assert_eq!(prefix(next_text), first_prefix);
            let projected = next.messages[2].content.as_str().unwrap();
            let data = metadata(projected);
            assert!(
                !projected.contains(&public_sha),
                "the metadata field remains an opaque stable identity"
            );
            assert_eq!(
                context
                    .restore_text(
                        data["agents"][0]["narrow_repair"]["source_sha256"]
                            .as_str()
                            .unwrap()
                    )
                    .unwrap(),
                public_sha
            );
            assert_ne!(
                context.mask_known(&private_sha).unwrap(),
                private_sha,
                "existing private digests stay private"
            );
            context
                .validate_wire(
                    &serde_json::to_vec(&json!({"messages":next.messages})).unwrap(),
                    revision,
                    &context.policy_fingerprint(),
                )
                .unwrap();
        });
    }

    #[test]
    fn privacy_native_digest_storage_stays_local_across_policy_reload_and_private_promotion() {
        crate::provider_filter::with_test_filter(&Default::default(), || {
            let (_root, context) = fixture();
            assert!(!context.selected_only);
            let target = context.workspace().join("target.py");
            std::fs::write(&target, "VALUE = 0\n").unwrap();
            let target = target.canonicalize().unwrap();
            let public_sha = "a".repeat(64);
            let private_sha = "c".repeat(64);
            context
                .project_source("secret.json", &json!({"secret":private_sha}).to_string())
                .unwrap();
            let ordinary_private = context.mask_known(&private_sha).unwrap();
            assert_ne!(ordinary_private, private_sha);
            let issue = |value: &str| TypedNativeControl {
                agents: vec![NativeAgentControl {
                    id: Uuid::new_v4(),
                    status: NativeAgentStatus::Running,
                    narrow_repair: Some(NativeNarrowRepairControl::issued_not_verified(
                        target.to_str().unwrap().into(),
                        "b".repeat(64),
                        value.into(),
                        None,
                    )),
                }],
                batch: None,
            };
            let source = context
                .record_native_control("private registration", issue(&public_sha))
                .unwrap();
            let data = metadata(
                &context
                    .project_retrieved("private registration", Some(&source))
                    .unwrap(),
            );
            let local_alias = data["agents"][0]["narrow_repair"]["source_sha256"]
                .as_str()
                .unwrap()
                .to_owned();
            assert!(local_alias.contains("_NATIVE_DIGEST_"));
            assert_eq!(context.mask_known(&public_sha).unwrap(), public_sha);
            let private_source = context
                .record_native_control("private second registration", issue(&private_sha))
                .unwrap();
            let private_data = metadata(
                &context
                    .project_retrieved("private second registration", Some(&private_source))
                    .unwrap(),
            );
            assert_ne!(
                private_data["agents"][0]["narrow_repair"]["source_sha256"],
                ordinary_private
            );
            assert_eq!(context.mask_known(&private_sha).unwrap(), ordinary_private);

            let mut policy = context.policy();
            policy.rules.push(crate::privacy::policy::Rule {
                file: "unused.json".into(),
                selector: "/**".into(),
                action: crate::privacy::policy::Action::Hide,
            });
            context.save_policy(policy).unwrap();
            assert_eq!(
                context.mask_known(&public_sha).unwrap(),
                public_sha,
                "policy installation cannot activate local metadata"
            );
            assert_ne!(context.mask_known(&private_sha).unwrap(), private_sha);
            let reference = context
                .state
                .lock()
                .unwrap()
                .registry
                .stage(&context.scope)
                .unwrap();
            let scope = context.scope.clone();
            let workspace = context.workspace.clone();
            let session = context.session_id;
            drop(context);
            let loaded =
                Context::open(scope.clone(), workspace.clone(), session, Some(reference)).unwrap();
            assert_eq!(
                loaded.mask_known(&public_sha).unwrap(),
                public_sha,
                "registry load cannot activate local metadata"
            );
            assert_ne!(loaded.mask_known(&private_sha).unwrap(), private_sha);
            assert_eq!(loaded.restore_text(&local_alias).unwrap(), public_sha);

            loaded
                .project_source(
                    "later.json",
                    &json!({"new_private_value":public_sha}).to_string(),
                )
                .unwrap();
            let promoted = loaded.mask_known(&public_sha).unwrap();
            assert_ne!(
                promoted, public_sha,
                "real source classification still makes this value private"
            );
            assert_ne!(
                promoted, local_alias,
                "local metadata cannot capture the later private content identity"
            );
            let reference = loaded
                .state
                .lock()
                .unwrap()
                .registry
                .stage(&loaded.scope)
                .unwrap();
            drop(loaded);
            let promoted_loaded =
                Context::open(scope, workspace, session, Some(reference)).unwrap();
            assert_ne!(promoted_loaded.mask_known(&public_sha).unwrap(), public_sha);
            assert_eq!(
                promoted_loaded.restore_text(&local_alias).unwrap(),
                public_sha
            );
        });
    }

    #[test]
    fn privacy_native_control_only_exact_host_template_exempts_private_status_words() {
        let (_root, context) = fixture();
        context
            .project_source("private.json", r#"{"field":"completed","name":"agent_id"}"#)
            .unwrap();
        let original = "private-control-canary completed";
        let source = context
            .record_native_control(
                original,
                control(Uuid::new_v4(), NativeAgentStatus::Completed),
            )
            .unwrap();
        let projected = context.project_retrieved(original, Some(&source)).unwrap();
        assert_eq!(context.mask_known(&projected).unwrap(), projected);
        assert_ne!(context.mask_known("completed").unwrap(), "completed");
        assert_ne!(
            context
                .mask_known(&format!("{projected}\ncompleted"))
                .unwrap(),
            format!("{projected}\ncompleted")
        );
        let body =
            serde_json::to_vec(&json!({"messages":[{"role":"tool","content":projected}]})).unwrap();
        context
            .validate_wire(
                &body,
                context.policy().revision,
                &context.policy_fingerprint(),
            )
            .unwrap();
        for ticket in context.state.lock().unwrap().safe_template_hashes.values() {
            ticket.revoke_for_test();
        }
        assert_ne!(context.mask_known(&projected).unwrap(), projected);
        let repaired = context.project_retrieved(original, Some(&source)).unwrap();
        assert_eq!(context.mask_known(&repaired).unwrap(), repaired);
        let mut policy = context.policy();
        policy.rules.push(crate::privacy::policy::Rule {
            file: "unused.json".into(),
            selector: "/**".into(),
            action: crate::privacy::policy::Action::Hide,
        });
        context.save_policy(policy).unwrap();
        assert_ne!(context.mask_known(&projected).unwrap(), projected);
        let fresh = context.project_retrieved(original, Some(&source)).unwrap();
        assert_eq!(context.mask_known(&fresh).unwrap(), fresh);
    }

    #[test]
    fn privacy_native_control_receipts_and_id_aliases_survive_registry_reload() {
        let (root, context) = fixture();
        let id = Uuid::new_v4();
        let batch = Uuid::new_v4();
        let original = "private result canary";
        let source = context
            .record_native_control(
                original,
                TypedNativeControl {
                    agents: vec![NativeAgentControl {
                        id,
                        status: NativeAgentStatus::Running,
                        narrow_repair: None,
                    }],
                    batch: Some(NativeAgentControl {
                        id: batch,
                        status: NativeAgentStatus::Queued,
                        narrow_repair: None,
                    }),
                },
            )
            .unwrap();
        let first = context.project_retrieved(original, Some(&source)).unwrap();
        let reference = context
            .state
            .lock()
            .unwrap()
            .registry
            .stage(&context.scope)
            .unwrap();
        let scope = context.scope.clone();
        let workspace = context.workspace.clone();
        let session = context.session_id;
        drop(context);
        let loaded = Context::open(scope, workspace, session, Some(reference)).unwrap();
        let next = loaded.project_retrieved(original, Some(&source)).unwrap();
        assert_eq!(metadata(&first), metadata(&next));
        let value = metadata(&next);
        assert_eq!(
            loaded
                .restore_text(value["batch"]["batch_id"].as_str().unwrap())
                .unwrap(),
            batch.to_string()
        );
        assert!(loaded.observed_sources().is_empty());
        assert!(root.path().exists());
    }

    #[test]
    fn privacy_native_control_rejects_unknown_or_invalid_metadata_and_never_borrows_file_allow() {
        let (_root, context) = fixture();
        assert!(context
            .record_native_control("body", control(Uuid::nil(), NativeAgentStatus::Running))
            .is_err());
        assert!(serde_json::from_str::<NativeAgentStatus>("\"private_unknown_status\"").is_err());
        let mut policy = context.policy();
        policy.rules.push(crate::privacy::policy::Rule {
            file: "**".into(),
            selector: "/**".into(),
            action: crate::privacy::policy::Action::Allow,
        });
        context.save_policy(policy).unwrap();
        let original = "unbound native private canary";
        let projected = context.project_retrieved(original, None).unwrap();
        assert!(!projected.contains(original));
    }
}
