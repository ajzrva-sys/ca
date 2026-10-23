//! Host-owned transient write-folder approvals.
use schemars::JsonSchema;
use serde::Deserialize;

pub const TOOL_NAME: &str = "request_write_access";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestWriteAccessArgs {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteAccessRequest {
    pub root: crate::workspace::SessionWriteRoot,
    pub reason: String,
}

impl WriteAccessRequest {
    pub fn prepare(arguments: &serde_json::Value) -> Result<Self, String> {
        let args: RequestWriteAccessArgs = serde_json::from_value(arguments.clone())
            .map_err(|error| format!("Invalid request_write_access: {error}"))?;
        if args.path.len() > 4096 || args.reason.trim().is_empty() || args.reason.len() > 512 {
            return Err("request_write_access needs a folder and a reason of 1-512 bytes".into());
        }
        Ok(Self {
            root: crate::workspace::SessionWriteRoot::prepare(std::path::Path::new(&args.path))
                .map_err(|error| error.to_string())?,
            reason: crate::terminal_text::sanitize_terminal_text(&args.reason),
        })
    }

    pub fn interaction(&self) -> crate::interaction::InteractionRequest {
        crate::interaction::InteractionRequest::from_model(crate::interaction::RequestUserInputArgs {
            question: crate::terminal_text::sanitize_terminal_text(&format!("Allow writes to {} for this session?", self.root.path.display())),
            description: Some(format!("{}. Native tools and sandboxed commands can write here until you switch sessions or exit ca. No administrator password is needed.", self.reason.trim_end_matches('.'))),
            kind: crate::interaction::ModelInteractionKind::Confirmation,
            choices: Vec::new(),
            allow_other: false,
        }).expect("host folder confirmation is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{PathOperation, WorkspaceContext};

    #[test]
    fn write_access_is_bound_to_folder_and_excluded_from_persisted_roots() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        let desktop = home.path().join("Desktop");
        let other = home.path().join("other");
        for directory in [&project, &desktop, &other] {
            std::fs::create_dir(directory).unwrap();
        }
        let mut workspace = WorkspaceContext::discover(&project).unwrap();
        let request = WriteAccessRequest::prepare(
            &serde_json::json!({"path":desktop,"reason":"Export requested report"}),
        )
        .unwrap();
        let report = desktop.join("report.txt");
        assert!(workspace
            .resolve(report.to_str().unwrap(), PathOperation::Create)
            .is_err());
        workspace.grant_session_write(request.root).unwrap();
        assert_eq!(
            workspace
                .resolve(report.to_str().unwrap(), PathOperation::Create)
                .unwrap(),
            desktop.canonicalize().unwrap().join("report.txt")
        );
        assert!(workspace
            .resolve(
                other.join("unapproved.txt").to_str().unwrap(),
                PathOperation::Create
            )
            .is_err());
        assert!(workspace.persisted_additional_roots().is_empty());
        assert_eq!(workspace.roots().len(), 1);
        assert_eq!(workspace.execution_roots().len(), 2);
        assert!(workspace
            .clone()
            .has_session_write(&desktop.canonicalize().unwrap()));
        workspace.clear_session_writes();
        assert!(!workspace.has_session_write(&desktop.canonicalize().unwrap()));
        assert_eq!(workspace.execution_roots().len(), 1);
    }

    #[test]
    fn write_access_rejects_replaced_folder_and_preserves_read_only_root() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("project");
        let output = home.path().join("out");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&output).unwrap();
        let mut workspace = WorkspaceContext::discover(&project).unwrap();
        let root = crate::workspace::SessionWriteRoot::prepare(&output).unwrap();
        std::fs::rename(&output, home.path().join("original")).unwrap();
        std::fs::create_dir(&output).unwrap();
        assert!(workspace.grant_session_write(root).is_err());
        workspace.add_root(&output, true).unwrap();
        assert!(workspace
            .grant_session_write(crate::workspace::SessionWriteRoot::prepare(&output).unwrap())
            .is_err());
        assert!(WriteAccessRequest::prepare(
            &serde_json::json!({"path":"relative", "reason":"export"})
        )
        .is_err());
        assert!(
            WriteAccessRequest::prepare(&serde_json::json!({"path":output, "reason":""})).is_err()
        );
    }

    #[tokio::test]
    async fn write_access_native_export_and_seatbelt_copy_use_user_permissions() {
        #[cfg(target_os = "macos")]
        let temporary_root = "/private/var/tmp";
        #[cfg(not(target_os = "macos"))]
        let temporary_root = "/var/tmp";
        let home = tempfile::Builder::new()
            .prefix("ca-export-")
            .tempdir_in(temporary_root)
            .unwrap();
        let project = home.path().join("project");
        let desktop = home.path().join("Desktop");
        std::fs::create_dir(&project).unwrap();
        std::fs::create_dir(&desktop).unwrap();
        let mut workspace = WorkspaceContext::discover(&project).unwrap();
        let owner = crate::jobs::JobOwner::new(None);
        let target = desktop.join("report.txt");
        let args = serde_json::json!({"path":target, "content":"exported"});
        assert!(crate::approval::ApprovalBroker::new(
            &workspace,
            &[],
            crate::approval::ApprovalOrigin::Main
        )
        .prepare(
            "write_file",
            &args,
            crate::agent::registry::ToolOrigin::BuiltIn
        )
        .is_err());
        workspace
            .grant_session_write(crate::workspace::SessionWriteRoot::prepare(&desktop).unwrap())
            .unwrap();
        let (prepared, _) = crate::approval::ApprovalBroker::new(
            &workspace,
            &[],
            crate::approval::ApprovalOrigin::Main,
        )
        .prepare(
            "write_file",
            &args,
            crate::agent::registry::ToolOrigin::BuiltIn,
        )
        .unwrap();
        let mut context = crate::agent::registry::ToolContext::native_read(
            &workspace,
            5,
            crate::agent::registry::ToolProfile::Main,
            crate::sandbox::SandboxMode::Required,
            owner,
        );
        let result = crate::agent::tools::execute_tool_with_context(
            "write_file",
            &prepared.arguments,
            &context,
        )
        .await;
        assert!(result.is_success(), "{}", result.model_text());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "exported");
        let copied = desktop.join("copy.txt");
        let command = format!("/bin/cp '{}' '{}'", target.display(), copied.display());
        let args = serde_json::json!({"command":command});
        let result =
            crate::agent::tools::execute_tool_with_context("run_command", &args, &context).await;
        assert!(result.is_success(), "{}", result.model_text());
        assert_eq!(std::fs::read_to_string(copied).unwrap(), "exported");
        workspace.clear_session_writes();
        context.workspace_roots = workspace.execution_roots();
        let denied = desktop.join("denied.txt");
        let args = serde_json::json!({"command":format!("/bin/cp '{}' '{}'", target.display(), denied.display())});
        let result =
            crate::agent::tools::execute_tool_with_context("run_command", &args, &context).await;
        assert!(!result.is_success());
        assert!(!denied.exists());
    }
}
