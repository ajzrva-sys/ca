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
    let fingerprint = context.policy_fingerprint();
    let mut state = context.state.lock().unwrap_or_else(|p| p.into_inner());
    let mut alias = |kind: &str, value: &str| -> Result<String, String> {
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
        .map(|control| {
            Ok(json!({"agent_id":alias("AGENT", &control.id.to_string())?,"status":control.status}))
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
        json!({"agents":agents,"batch":batch,"body":body})
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
            agents: vec![NativeAgentControl { id, status }],
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
                    }],
                    batch: Some(NativeAgentControl {
                        id: batch,
                        status: NativeAgentStatus::Queued,
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
