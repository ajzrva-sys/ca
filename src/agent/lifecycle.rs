#![allow(dead_code)] // public boundary for optional trusted plugin adapters

use super::subagent::{
    AgentId, AgentManager, AgentManagerError, AgentRunOrigin, AgentSnapshot, AgentSpawnRequest,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub const AGENT_LIFECYCLE_VERSION: u32 = 1;
const TERMINAL_RETENTION: Duration = Duration::hours(1);
const MAX_CORRELATION_BYTES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentLifecycleHandle {
    pub version: u32,
    pub agent_id: AgentId,
    pub origin_turn: Uuid,
    pub correlation_id: String,
    pub issued_at: DateTime<Utc>,
    pub capability: String,
}

#[derive(Debug, Clone)]
struct LifecycleRecord {
    handle: AgentLifecycleHandle,
}

#[derive(Debug, Default)]
struct LifecycleState {
    by_correlation: HashMap<(Uuid, String), LifecycleRecord>,
}

#[derive(Clone)]
pub struct AgentLifecycleService {
    manager: Arc<AgentManager>,
    secret: [u8; 32],
    state: Arc<Mutex<LifecycleState>>,
    terminal_retention: Duration,
}

impl std::fmt::Debug for AgentLifecycleService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentLifecycleService")
            .field("version", &AGENT_LIFECYCLE_VERSION)
            .field("secret", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentLifecycleError {
    #[error("invalid lifecycle correlation ID")]
    InvalidCorrelation,
    #[error("invalid or forged lifecycle handle")]
    InvalidHandle,
    #[error("lifecycle handle belongs to a different origin")]
    CrossOrigin,
    #[error("lifecycle terminal record expired")]
    Expired,
    #[error("agent has not reached a terminal state")]
    NotTerminal,
    #[error(transparent)]
    Manager(#[from] AgentManagerError),
}

impl AgentLifecycleService {
    pub fn new(manager: Arc<AgentManager>) -> Self {
        let left = Uuid::new_v4();
        let right = Uuid::new_v4();
        let mut secret = [0_u8; 32];
        secret[..16].copy_from_slice(left.as_bytes());
        secret[16..].copy_from_slice(right.as_bytes());
        Self {
            manager,
            secret,
            state: Arc::new(Mutex::new(LifecycleState::default())),
            terminal_retention: TERMINAL_RETENTION,
        }
    }

    #[cfg(test)]
    fn with_terminal_retention(manager: Arc<AgentManager>, terminal_retention: Duration) -> Self {
        Self {
            terminal_retention,
            ..Self::new(manager)
        }
    }

    pub async fn launch(
        &self,
        origin_turn: Uuid,
        correlation_id: impl Into<String>,
        request: AgentSpawnRequest,
    ) -> Result<AgentLifecycleHandle, AgentLifecycleError> {
        let correlation_id = validate_correlation(correlation_id.into())?;
        if request.parent_turn != origin_turn {
            return Err(AgentLifecycleError::CrossOrigin);
        }
        self.purge_expired().await;
        let key = (origin_turn, correlation_id.clone());
        let mut state = self.state.lock().await;
        if let Some(record) = state.by_correlation.get(&key) {
            return Ok(record.handle.clone());
        }
        let agent_id = self
            .manager
            .spawn_with_origin(request, AgentRunOrigin::Conversation)
            .await?;
        let issued_at = Utc::now();
        let capability = self.capability(agent_id, origin_turn, &correlation_id, issued_at);
        let handle = AgentLifecycleHandle {
            version: AGENT_LIFECYCLE_VERSION,
            agent_id,
            origin_turn,
            correlation_id,
            issued_at,
            capability,
        };
        state.by_correlation.insert(
            key,
            LifecycleRecord {
                handle: handle.clone(),
            },
        );
        Ok(handle)
    }

    pub async fn reconnect(
        &self,
        origin_turn: Uuid,
        correlation_id: &str,
    ) -> Result<AgentLifecycleHandle, AgentLifecycleError> {
        let correlation_id = validate_correlation(correlation_id.to_string())?;
        self.purge_expired().await;
        self.state
            .lock()
            .await
            .by_correlation
            .get(&(origin_turn, correlation_id))
            .map(|record| record.handle.clone())
            .ok_or(AgentLifecycleError::Expired)
    }

    pub async fn status(
        &self,
        origin_turn: Uuid,
        handle: &AgentLifecycleHandle,
    ) -> Result<AgentSnapshot, AgentLifecycleError> {
        self.validate_handle(origin_turn, handle).await?;
        self.manager
            .snapshot(handle.agent_id)
            .await
            .map_err(Into::into)
    }

    pub async fn wait(
        &self,
        origin_turn: Uuid,
        handle: &AgentLifecycleHandle,
        timeout_secs: Option<u64>,
    ) -> Result<AgentSnapshot, AgentLifecycleError> {
        self.validate_handle(origin_turn, handle).await?;
        self.manager
            .await_agent_bounded(handle.agent_id, timeout_secs)
            .await
            .map_err(Into::into)
    }

    pub async fn cancel(
        &self,
        origin_turn: Uuid,
        handle: &AgentLifecycleHandle,
    ) -> Result<(), AgentLifecycleError> {
        self.validate_handle(origin_turn, handle).await?;
        self.manager
            .cancel(handle.agent_id)
            .await
            .map_err(Into::into)
    }

    pub async fn result(
        &self,
        origin_turn: Uuid,
        handle: &AgentLifecycleHandle,
    ) -> Result<AgentSnapshot, AgentLifecycleError> {
        let snapshot = self.status(origin_turn, handle).await?;
        if !snapshot.status.terminal() {
            return Err(AgentLifecycleError::NotTerminal);
        }
        Ok(snapshot)
    }

    async fn validate_handle(
        &self,
        origin_turn: Uuid,
        handle: &AgentLifecycleHandle,
    ) -> Result<(), AgentLifecycleError> {
        if origin_turn != handle.origin_turn {
            return Err(AgentLifecycleError::CrossOrigin);
        }
        if handle.version != AGENT_LIFECYCLE_VERSION
            || self.capability(
                handle.agent_id,
                handle.origin_turn,
                &handle.correlation_id,
                handle.issued_at,
            ) != handle.capability
        {
            return Err(AgentLifecycleError::InvalidHandle);
        }
        self.purge_expired().await;
        let state = self.state.lock().await;
        let Some(record) = state
            .by_correlation
            .get(&(handle.origin_turn, handle.correlation_id.clone()))
        else {
            return Err(AgentLifecycleError::Expired);
        };
        if record.handle != *handle {
            return Err(AgentLifecycleError::InvalidHandle);
        }
        Ok(())
    }

    async fn purge_expired(&self) {
        let now = Utc::now();
        let records = {
            let state = self.state.lock().await;
            state
                .by_correlation
                .iter()
                .map(|(key, record)| (key.clone(), record.handle.agent_id))
                .collect::<Vec<_>>()
        };
        let mut expired = Vec::new();
        for (key, agent_id) in records {
            if let Ok(snapshot) = self.manager.snapshot(agent_id).await {
                if snapshot.status.terminal() && snapshot.updated_at + self.terminal_retention < now
                {
                    expired.push(key);
                }
            }
        }
        if !expired.is_empty() {
            let mut state = self.state.lock().await;
            for key in expired {
                state.by_correlation.remove(&key);
            }
        }
    }
    fn capability(
        &self,
        agent_id: AgentId,
        origin_turn: Uuid,
        correlation_id: &str,
        issued_at: DateTime<Utc>,
    ) -> String {
        let mut digest = Sha256::new();
        digest.update(self.secret);
        digest.update(AGENT_LIFECYCLE_VERSION.to_be_bytes());
        digest.update(agent_id.0.as_bytes());
        digest.update(origin_turn.as_bytes());
        digest.update((correlation_id.len() as u64).to_be_bytes());
        digest.update(correlation_id.as_bytes());
        digest.update(issued_at.timestamp_micros().to_be_bytes());
        format!("{:x}", digest.finalize())
    }
}

fn validate_correlation(value: String) -> Result<String, AgentLifecycleError> {
    let value = value.trim().to_string();
    if value.is_empty()
        || value.len() > MAX_CORRELATION_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(AgentLifecycleError::InvalidCorrelation);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::subagent::{
        AgentContextMode, AgentGrant, AgentIsolation, AgentLimits, AgentRunContext,
        AgentRunPurpose, AgentRunner,
    };
    use async_trait::async_trait;

    struct Runner;

    #[async_trait]
    impl AgentRunner for Runner {
        async fn run(&self, _context: AgentRunContext) -> Result<String, String> {
            Ok("done".into())
        }
    }

    fn request(turn: Uuid) -> AgentSpawnRequest {
        AgentSpawnRequest {
            parent: None,
            parent_turn: turn,
            task_id: None,
            task: "lifecycle".into(),
            context_mode: AgentContextMode::Summary,
            context_summary: "bounded".into(),
            grants: AgentGrant {
                tools: vec!["read_file".into()],
                workspace_root_ids: vec!["primary".into()],
                path_scopes: Vec::new(),
                command_scopes: Vec::new(),
                network: false,
            },
            workspace: crate::workspace::WorkspaceContext::discover("/tmp").unwrap(),
            depth: 1,
            route: None,
            purpose: AgentRunPurpose::Subagent,
            output_contract: None,
            isolation: AgentIsolation::Shared,
        }
    }

    #[tokio::test]
    async fn handles_are_capability_protected_origin_bound_and_correlated() {
        let manager = AgentManager::new(Arc::new(Runner), AgentLimits::default());
        let service = AgentLifecycleService::new(manager);
        let turn = Uuid::new_v4();
        let handle = service
            .launch(turn, "plugin:job-1", request(turn))
            .await
            .unwrap();
        let repeated = service
            .launch(turn, "plugin:job-1", request(turn))
            .await
            .unwrap();
        assert_eq!(handle, repeated);
        assert_eq!(
            service.reconnect(turn, "plugin:job-1").await.unwrap(),
            handle
        );
        assert!(matches!(
            service.status(Uuid::new_v4(), &handle).await,
            Err(AgentLifecycleError::CrossOrigin)
        ));
        let mut forged = handle.clone();
        let replacement = if forged.capability.starts_with('0') {
            "1"
        } else {
            "0"
        };
        forged.capability.replace_range(..1, replacement);
        assert!(matches!(
            service.status(turn, &forged).await,
            Err(AgentLifecycleError::InvalidHandle)
        ));
        let terminal = service.wait(turn, &handle, None).await.unwrap();
        assert!(terminal.status.terminal());
        assert!(service.result(turn, &handle).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_launches_with_one_correlation_admit_only_one_child() {
        let manager = AgentManager::new(Arc::new(Runner), AgentLimits::default());
        let service = AgentLifecycleService::new(manager.clone());
        let turn = Uuid::new_v4();
        let left = service.launch(turn, "plugin:concurrent", request(turn));
        let right = service.launch(turn, "plugin:concurrent", request(turn));
        let (left, right) = tokio::join!(left, right);
        assert_eq!(left.unwrap(), right.unwrap());
        assert_eq!(manager.list().await.len(), 1);
    }

    #[tokio::test]
    async fn lifecycle_launch_obeys_global_spawn_pause() {
        let manager = AgentManager::new(Arc::new(Runner), AgentLimits::default());
        manager.set_spawns_paused(true);
        let service = AgentLifecycleService::new(manager);
        let turn = Uuid::new_v4();
        assert!(matches!(
            service.launch(turn, "paused", request(turn)).await,
            Err(AgentLifecycleError::Manager(
                AgentManagerError::SpawnsPaused
            ))
        ));
    }

    #[tokio::test]
    async fn terminal_lifecycle_records_expire_after_the_retention_window() {
        let manager = AgentManager::new(Arc::new(Runner), AgentLimits::default());
        let service = AgentLifecycleService::with_terminal_retention(manager, Duration::zero());
        let turn = Uuid::new_v4();
        let handle = service
            .launch(turn, "expires", request(turn))
            .await
            .unwrap();
        service.wait(turn, &handle, None).await.unwrap();
        assert!(matches!(
            service.reconnect(turn, "expires").await,
            Err(AgentLifecycleError::Expired)
        ));
        assert!(matches!(
            service.status(turn, &handle).await,
            Err(AgentLifecycleError::Expired)
        ));
    }
}
