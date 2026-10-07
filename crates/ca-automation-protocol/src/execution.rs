//! Host-owned execution identity. Legacy V1 workflow contracts stay unchanged.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const EXECUTION_SCHEMA: &str = "ca.automation-execution.v2";
pub const RECEIPT_SCHEMA: &str = "ca.automation-execution-receipt.v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    pub schema: String,
    pub run_id: String,
    pub profile: String,
    pub state_root: PathBuf,
    pub lease_root: PathBuf,
    pub definition_digest: String,
    pub resource_binding_digest: String,
    pub policy_digest: String,
    pub resources: Vec<String>,
    pub runtime_seconds: u64,
    pub output_bytes: u64,
}

impl ExecutionContext {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_SCHEMA
            || !valid_id(&self.run_id)
            || !valid_id(&self.profile)
            || !self.state_root.is_absolute()
            || !self.lease_root.is_absolute()
            || self
                .state_root
                .components()
                .chain(self.lease_root.components())
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || [
                &self.definition_digest,
                &self.resource_binding_digest,
                &self.policy_digest,
            ]
            .iter()
            .any(|d| !valid_digest(d))
            || self.runtime_seconds == 0
            || self.runtime_seconds > 86_400
            || self.output_bytes == 0
            || self.output_bytes > 16 * 1024 * 1024
            || self.resources.len() > 64
            || self
                .resources
                .iter()
                .any(|r| r.is_empty() || r.len() > 4096 || r.contains('\0'))
        {
            return Err("invalid automation execution context".into());
        }
        Ok(())
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Prepared,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    NeedsAttention,
}
impl ExecutionState {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Prepared | Self::Running)
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Prepared,
    Running,
    Confirmed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionReceipt {
    pub id: String,
    pub state: ActionState,
    pub result: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub schema: String,
    pub run_id: String,
    pub definition_digest: String,
    pub resource_binding_digest: String,
    pub policy_digest: String,
    pub state: ExecutionState,
    pub actions: Vec<ActionReceipt>,
    pub result: Option<Value>,
    pub reason: Option<String>,
}
