//! Explicit execution ownership captured by a tool worker, never inferred from focus.
use std::future::Future;

#[derive(Debug, Clone, Default)]
pub struct OwnedToolContext {
    pub discovery: crate::tool_discovery::ToolDiscoverySession,
    pub recovery: Option<crate::recovery::RecoveryContext>,
    pub automation: Option<crate::automation_ui::AutomationToolScope>,
    pub plan_workflow_source: Option<crate::plan_workflow::WorkflowRequestSource>,
}

tokio::task_local! {
    static OWNED: OwnedToolContext;
}

pub fn current_recovery() -> Option<crate::recovery::RecoveryContext> {
    OWNED
        .try_with(|context| context.recovery.clone())
        .ok()
        .flatten()
}

pub fn current_discovery() -> Option<crate::tool_discovery::ToolDiscoverySession> {
    OWNED.try_with(|context| context.discovery.clone()).ok()
}

pub fn current_automation() -> Option<crate::automation_ui::AutomationToolScope> {
    OWNED
        .try_with(|context| context.automation.clone())
        .ok()
        .flatten()
}

pub fn current_plan_workflow_source() -> Option<crate::plan_workflow::WorkflowRequestSource> {
    OWNED
        .try_with(|context| context.plan_workflow_source.clone())
        .ok()
        .flatten()
}

pub async fn with_recovery<F: Future>(
    context: Option<crate::recovery::RecoveryContext>,
    operation: F,
) -> F::Output {
    OWNED
        .scope(
            OwnedToolContext {
                recovery: context,
                ..Default::default()
            },
            operation,
        )
        .await
}

pub fn spawn_owned<F>(context: OwnedToolContext, operation: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(OWNED.scope(context, operation))
}
