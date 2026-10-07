use crate::config::ApprovalRule;
use glob::Pattern;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // review/agent/MCP consumers are introduced by later ordered tasks
pub enum ApprovalOrigin {
    Main,
    Review,
    Agent(String),
    Mcp(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerDecision {
    AllowedByRule,
    DeniedByRule,
    ExplicitPrompt,
    Prompt,
    NetworkPrompt,
}

#[derive(Debug, Clone)]
pub struct ApprovalBroker<'a> {
    workspace: &'a crate::workspace::WorkspaceContext,
    rules: &'a [ApprovalRule],
    permission_rules: &'a [crate::permission::PermissionRule],
    session_grants: &'a [crate::permission::PermissionMatcher],
    #[allow(dead_code)]
    pub origin: ApprovalOrigin,
}

impl<'a> ApprovalBroker<'a> {
    #[allow(dead_code)] // compatibility constructor for legacy callers and tests
    pub fn new(
        workspace: &'a crate::workspace::WorkspaceContext,
        rules: &'a [ApprovalRule],
        origin: ApprovalOrigin,
    ) -> Self {
        Self {
            workspace,
            rules,
            permission_rules: &[],
            session_grants: &[],
            origin,
        }
    }

    pub fn with_policy(
        workspace: &'a crate::workspace::WorkspaceContext,
        rules: &'a [ApprovalRule],
        permission_rules: &'a [crate::permission::PermissionRule],
        session_grants: &'a [crate::permission::PermissionMatcher],
        origin: ApprovalOrigin,
    ) -> Self {
        Self {
            workspace,
            rules,
            permission_rules,
            session_grants,
            origin,
        }
    }

    pub fn prepare(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        tool_origin: crate::agent::registry::ToolOrigin,
    ) -> Result<(crate::workspace::PreparedToolInvocation, BrokerDecision), String> {
        self.prepare_with_combined_network_decision(name, arguments, tool_origin, false)
    }

    /// Prepare and classify an invocation on a surface that either handles
    /// network as a separate gate or includes it in the tool decision. Combining
    /// the decisions suppresses only the extra network prompt; Deny and explicit
    /// Ask remain ceilings.
    pub fn prepare_with_combined_network_decision(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        tool_origin: crate::agent::registry::ToolOrigin,
        combine_network: bool,
    ) -> Result<(crate::workspace::PreparedToolInvocation, BrokerDecision), String> {
        let prepared =
            crate::workspace::prepare_tool_invocation(self.workspace, name, arguments, tool_origin)
                .map_err(|error| error.to_string())?;
        let rule_decision = crate::permission::evaluate(
            name,
            &prepared.arguments,
            self.rules,
            self.permission_rules,
            self.session_grants,
        );
        let decision = match rule_decision {
            crate::permission::PermissionOutcome::Deny => BrokerDecision::DeniedByRule,
            crate::permission::PermissionOutcome::Ask => BrokerDecision::ExplicitPrompt,
            _ if crate::permission::host_authorized_workflow_shape(name, &prepared.arguments) => {
                BrokerDecision::AllowedByRule
            }
            _ if prepared.network_requested && !combine_network => BrokerDecision::NetworkPrompt,
            crate::permission::PermissionOutcome::Allow => BrokerDecision::AllowedByRule,
            crate::permission::PermissionOutcome::NoMatch => BrokerDecision::Prompt,
        };
        Ok((prepared, decision))
    }

    pub fn prepare_dynamic(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        tool_origin: crate::agent::registry::ToolOrigin,
        network_requested: bool,
    ) -> (crate::workspace::PreparedToolInvocation, BrokerDecision) {
        let prepared = crate::workspace::PreparedToolInvocation {
            name: name.to_string(),
            arguments: arguments.clone(),
            origin: tool_origin,
            network_requested,
        };
        let rule_decision = crate::permission::evaluate(
            name,
            arguments,
            self.rules,
            self.permission_rules,
            self.session_grants,
        );
        let decision = match rule_decision {
            crate::permission::PermissionOutcome::Deny => BrokerDecision::DeniedByRule,
            crate::permission::PermissionOutcome::Ask => BrokerDecision::ExplicitPrompt,
            _ if crate::permission::host_authorized_workflow_shape(name, arguments) => {
                BrokerDecision::AllowedByRule
            }
            _ if network_requested => BrokerDecision::NetworkPrompt,
            crate::permission::PermissionOutcome::Allow => BrokerDecision::AllowedByRule,
            crate::permission::PermissionOutcome::NoMatch => BrokerDecision::Prompt,
        };
        (prepared, decision)
    }
}

/// Check if a tool call matches a specific approval rule
#[allow(dead_code)] // legacy rule compatibility surface
pub fn matches_rule(
    tool_name: &str,
    tool_args: &serde_json::Value,
    cwd: &str,
    rule: &ApprovalRule,
) -> bool {
    match rule {
        ApprovalRule::AllowTool(t) | ApprovalRule::DenyTool(t) => t == tool_name,
        ApprovalRule::AllowPath { tool, path_pattern }
        | ApprovalRule::DenyPath { tool, path_pattern } => {
            if tool != tool_name {
                return false;
            }
            // Extract path from arguments (most tools use "path", shell uses "cwd")
            let path = tool_args["path"]
                .as_str()
                .or_else(|| tool_args["cwd"].as_str())
                .unwrap_or(cwd);
            let resolved = crate::agent::tools::resolve_path(path, cwd);
            Pattern::new(path_pattern)
                .map(|p| p.matches(&resolved))
                .unwrap_or(false)
        }
    }
}

/// Check approval rules against a tool call.
/// Returns Some(true) if allowed by rule, Some(false) if denied, None if no rule matches (fall through to prompt).
#[allow(dead_code)] // legacy rule compatibility surface
pub fn check_approval(
    tool_name: &str,
    tool_args: &serde_json::Value,
    cwd: &str,
    rules: &[ApprovalRule],
) -> Option<bool> {
    // Deny rules take highest priority
    for rule in rules {
        if matches!(
            rule,
            ApprovalRule::DenyTool(_) | ApprovalRule::DenyPath { .. }
        ) && matches_rule(tool_name, tool_args, cwd, rule)
        {
            return Some(false);
        }
    }
    // Then allow rules
    for rule in rules {
        if matches!(
            rule,
            ApprovalRule::AllowTool(_) | ApprovalRule::AllowPath { .. }
        ) && matches_rule(tool_name, tool_args, cwd, rule)
        {
            return Some(true);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApprovalRule;

    // ─── M1: ApprovalQueue model tests ────────────────────────────

    use crate::app::{ApprovalQueue, PendingApproval, ResolutionAction};

    fn pa(name: &str, id: &str) -> PendingApproval {
        PendingApproval {
            tool_name: name.into(),
            tool_args: "{}".into(),
            tool_id: id.into(),
            network_requested: false,
            email_preview: None,
            email_draft_ref: None,
        }
    }

    #[test]
    fn queue_empty_initially() {
        let q = ApprovalQueue::new();
        assert!(!q.is_active());
        assert_eq!(q.progress(), (0, 0));
    }

    #[test]
    fn queue_advance_empty_returns_false() {
        let mut q = ApprovalQueue::new();
        assert!(!q.advance());
        assert!(q.current().is_none());
    }

    #[test]
    fn queue_advance_single_item() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("write_file", "call-1")]);
        assert!(q.current().is_some());
        assert_eq!(q.current().unwrap().tool_name, "write_file");
        // After begin_batch with one item, it should be in current
        // and remaining should be empty (progress shows 0 of 1)
        assert_eq!(q.progress(), (0, 1));
        // Resolving should exhaust the queue
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn queue_advance_sequential_ordering() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![
            pa("write_file", "c1"),
            pa("run_command", "c2"),
            pa("edit_file", "c3"),
        ]);

        let mut names = Vec::new();
        names.push(q.current().unwrap().tool_name.clone());
        // Resolve first, get next
        if let ResolutionAction::Advance { next, .. } = q.resolve() {
            names.push(next.tool_name.clone());
        }
        if let ResolutionAction::Advance { next, .. } = q.resolve() {
            names.push(next.tool_name.clone());
        }
        assert_eq!(names, vec!["write_file", "run_command", "edit_file"]);
    }

    #[test]
    fn queue_active_with_current_or_remaining() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "1"), pa("b", "2")]);
        assert!(q.is_active()); // has current after begin_batch

        // Resolve first - should advance to second
        let action = q.resolve();
        assert!(q.is_active()); // still has remaining
        assert!(matches!(action, ResolutionAction::Advance { .. }));
    }

    #[test]
    fn queue_remaining_only_is_active() {
        // After begin_batch, current is set from remaining.
        // Test that is_active is true when items exist.
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "1"), pa("b", "2")]);
        assert!(q.is_active());
        // Clear and verify empty state
        q.clear();
        assert!(!q.is_active());
    }

    #[test]
    fn queue_progress_accounting() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "1"), pa("b", "2"), pa("c", "3")]);
        assert_eq!(q.progress(), (0, 3));

        q.resolve(); // resolve first
        assert_eq!(q.progress(), (1, 3));

        q.resolve(); // resolve second
        assert_eq!(q.progress(), (2, 3));

        let action = q.resolve(); // resolve third
        assert_eq!(q.progress(), (3, 3));
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn queue_exhaustion_advances_through_all() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("x", "1"), pa("y", "2")]);
        assert!(q.is_active());

        let action = q.resolve();
        assert!(q.is_active());
        assert!(matches!(action, ResolutionAction::Advance { .. }));

        let action = q.resolve();
        assert!(!q.is_active());
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn queue_clean_batch_reset_and_reuse() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "1")]);
        assert!(q.is_active());

        q.resolve();
        assert!(!q.is_active());

        // Reuse with a new batch
        q.begin_batch(vec![pa("d", "4"), pa("e", "5")]);
        assert!(q.is_active());
        assert_eq!(q.current().unwrap().tool_name, "d");
    }

    #[test]
    fn queue_default_matches_new() {
        let q1 = ApprovalQueue::new();
        let q2 = ApprovalQueue::default();
        assert!(!q1.is_active());
        assert!(!q2.is_active());
    }

    // ─── M2: Batch classification and async orchestration tests ──

    #[test]
    fn m2_begin_batch_populates_atomically() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![
            pa("tool_a", "c1"),
            pa("tool_b", "c2"),
            pa("tool_c", "c3"),
        ]);
        assert_eq!(q.progress(), (0, 3));
        assert!(q.is_active());
        assert_eq!(q.current().unwrap().tool_id, "c1");
        // After begin_batch with 3 items, first is current, 2 remain
        // progress shows 0 resolved of 3 total
        assert_eq!(q.progress(), (0, 3));
    }

    #[test]
    fn m2_begin_batch_clears_previous_state() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("old", "old-1")]);
        q.mark_in_flight();

        // New batch should clear everything
        q.begin_batch(vec![pa("new", "new-1")]);
        assert!(!q.is_in_flight());
        assert_eq!(q.progress(), (0, 1));
        assert_eq!(q.current().unwrap().tool_id, "new-1");
    }

    #[test]
    fn m2_fifo_two_plus_decisions() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![
            pa("first", "c1"),
            pa("second", "c2"),
            pa("third", "c3"),
        ]);

        // First decision
        assert_eq!(q.current().unwrap().tool_id, "c1");
        let action = q.resolve();
        match action {
            ResolutionAction::Advance {
                next,
                resolved,
                total,
            } => {
                assert_eq!(next.tool_id, "c2");
                assert_eq!(resolved, 1);
                assert_eq!(total, 3);
            }
            _ => panic!("Expected Advance"),
        }

        // Second decision
        assert_eq!(q.current().unwrap().tool_id, "c2");
        let action = q.resolve();
        match action {
            ResolutionAction::Advance {
                next,
                resolved,
                total,
            } => {
                assert_eq!(next.tool_id, "c3");
                assert_eq!(resolved, 2);
                assert_eq!(total, 3);
            }
            _ => panic!("Expected Advance"),
        }

        // Third decision (last)
        assert_eq!(q.current().unwrap().tool_id, "c3");
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
        assert!(!q.is_active());
    }

    #[test]
    fn m2_duplicate_prevention_while_in_flight() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("tool_a", "c1")]);

        // Simulate user pressing 'y': mark in_flight
        q.mark_in_flight();
        assert!(q.is_in_flight());
        assert!(q.is_active()); // still active while in flight

        // Duplicate keypress should be prevented by is_in_flight check
        // (caller should check is_in_flight before acting)
        assert!(q.is_in_flight());

        // Async task completes, clear in_flight and resolve
        q.clear_in_flight();
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn m2_resolve_returns_advance_for_next_item() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);

        let action = q.resolve();
        match action {
            ResolutionAction::Advance {
                next,
                resolved,
                total,
            } => {
                assert_eq!(next.tool_id, "c2");
                assert_eq!(next.tool_name, "b");
                assert_eq!(resolved, 1);
                assert_eq!(total, 2);
            }
            _ => panic!("Expected Advance"),
        }
    }

    #[test]
    fn m2_resolve_returns_continue_when_exhausted() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);

        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
        assert!(!q.is_active());
    }

    #[test]
    fn m2_mixed_persistent_rules_classification() {
        // Test that check_approval correctly classifies tools
        let rules = vec![
            ApprovalRule::AllowTool("read_file".into()),
            ApprovalRule::DenyTool("run_command".into()),
        ];

        // read_file → allowed by rule (should not enter queue)
        let args = serde_json::json!({"path": "src/main.rs"});
        assert_eq!(
            check_approval("read_file", &args, "/tmp", &rules),
            Some(true)
        );

        // run_command → denied by rule (should not enter queue)
        let args = serde_json::json!({"command": "ls"});
        assert_eq!(
            check_approval("run_command", &args, "/tmp", &rules),
            Some(false)
        );

        // write_file → no rule matches (should enter queue)
        let args = serde_json::json!({"path": "out.txt"});
        assert_eq!(check_approval("write_file", &args, "/tmp", &rules), None);
    }

    #[test]
    fn m2_pre_resolved_tools_not_in_queue() {
        // Simulate what execute_pending_tools_async should do:
        // pre-resolved (allowed/denied by rule) tools produce results but not queue entries
        let rules = vec![
            ApprovalRule::AllowTool("read_file".into()),
            ApprovalRule::DenyTool("run_command".into()),
        ];

        // Simulate classification of 3 tools: read_file (allow), run_command (deny), write_file (prompt)
        let mut pre_resolved: Vec<(String, String)> = Vec::new(); // (tool_id, output)
        let mut pending: Vec<PendingApproval> = Vec::new();

        let tools = [
            ("c1", "read_file", r#"{"path":"src/main.rs"}"#),
            ("c2", "run_command", r#"{"command":"ls"}"#),
            ("c3", "write_file", r#"{"path":"out.txt"}"#),
        ];

        for (id, name, args_str) in &tools {
            let args: serde_json::Value = serde_json::from_str(args_str).unwrap();
            match check_approval(name, &args, "/tmp", &rules) {
                Some(true) => {
                    pre_resolved.push((id.to_string(), format!("auto-executed: {}", name)));
                }
                Some(false) => {
                    pre_resolved.push((
                        id.to_string(),
                        "Denied by persistent approval rule".to_string(),
                    ));
                }
                None => {
                    pending.push(PendingApproval {
                        tool_name: name.to_string(),
                        tool_args: args_str.to_string(),
                        tool_id: id.to_string(),
                        network_requested: false,
                        email_preview: None,
                        email_draft_ref: None,
                    });
                }
            }
        }

        // Only write_file should be in pending
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].tool_name, "write_file");

        // Pre-resolved results should NOT include pending tools
        assert_eq!(pre_resolved.len(), 2);
        assert!(pre_resolved.iter().any(|(id, _)| id == "c1"));
        assert!(pre_resolved.iter().any(|(id, _)| id == "c2"));
        assert!(!pre_resolved.iter().any(|(id, _)| id == "c3"));
    }

    #[test]
    fn m2_no_empty_unresolved_tool_results() {
        // ToolResults sent to provider must never contain empty strings for pending tools
        let rules: Vec<ApprovalRule> = vec![];
        let mut pre_resolved: Vec<(String, String)> = Vec::new();
        let mut pending: Vec<PendingApproval> = Vec::new();

        let tools = [
            ("c1", "read_file", r#"{"path":"a.rs"}"#),
            ("c2", "write_file", r#"{"path":"b.rs"}"#),
        ];

        for (id, name, args_str) in &tools {
            let args: serde_json::Value = serde_json::from_str(args_str).unwrap();
            match check_approval(name, &args, "/tmp", &rules) {
                Some(true) => {
                    pre_resolved.push((id.to_string(), "executed".to_string()));
                }
                Some(false) => {
                    pre_resolved.push((id.to_string(), "Denied".to_string()));
                }
                None => {
                    pending.push(PendingApproval {
                        tool_name: name.to_string(),
                        tool_args: args_str.to_string(),
                        tool_id: id.to_string(),
                        network_requested: false,
                        email_preview: None,
                        email_draft_ref: None,
                    });
                }
            }
        }

        // read_file not in allowlist rules, so it goes to pending too (no rules configured)
        // Both tools go to pending when no rules
        assert_eq!(pending.len(), 2);
        assert_eq!(pre_resolved.len(), 0);

        // Pre-resolved results must never contain empty/placeholder outputs
        for (_, output) in &pre_resolved {
            assert!(!output.is_empty(), "pre-resolved output must not be empty");
        }
    }

    #[test]
    fn m2_explicit_denial_produces_error_status() {
        use crate::app::{Message, Role, ToolCall, ToolStatus};

        let mut state = crate::app::AppState {
            cwd: "/tmp/ca-test".to_string(),
            ..Default::default()
        };
        state.messages.push(Message {
            role: Role::Assistant,
            content: "".into(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "run_command".into(),
                arguments: "{}".into(),
                status: ToolStatus::Running,
                output: None,
            }]),
            thinking: None,
            timestamp: chrono::Utc::now(),
            response_items: Vec::new(),
        });

        // Simulate denial: write "Denied" result with Error status
        if let Some(last) = state.messages.last_mut() {
            if let Some(ref mut tcs) = last.tool_calls {
                for tc in tcs.iter_mut() {
                    if tc.id == "c1" {
                        tc.output = Some("Denied".into());
                        tc.status = ToolStatus::Error;
                        break;
                    }
                }
            }
        }

        let msg = state.messages.last().unwrap();
        let tc = &msg.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.output.as_deref(), Some("Denied"));
        assert_eq!(tc.status, ToolStatus::Error);
    }

    #[test]
    fn m2_exactly_once_continuation_after_final_resolution() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);

        // First resolution → Advance
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Advance { .. }));

        // Second resolution → Continue (exactly once)
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));

        // No more resolutions possible
        assert!(!q.is_active());
    }

    #[test]
    fn m2_queue_clear_and_reuse() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);
        q.mark_in_flight();

        q.clear();
        assert!(!q.is_active());
        assert!(!q.is_in_flight());
        assert_eq!(q.progress(), (0, 0));

        // Reuse
        q.begin_batch(vec![pa("c", "c3")]);
        assert!(q.is_active());
        assert_eq!(q.current().unwrap().tool_id, "c3");
    }

    #[test]
    fn m2_cancellation_equivalent_to_denial() {
        // Esc should produce same outcome as 'n': tool gets Denied, Error status
        // This is a logical test - the actual key mapping is in main.rs
        use crate::app::{Message, Role, ToolCall, ToolStatus};

        let mut state = crate::app::AppState {
            cwd: "/tmp/ca-test".to_string(),
            ..Default::default()
        };
        state.messages.push(Message {
            role: Role::Assistant,
            content: "".into(),
            tool_calls: Some(vec![ToolCall {
                id: "c1".into(),
                name: "run_command".into(),
                arguments: "{}".into(),
                status: ToolStatus::Running,
                output: None,
            }]),
            thinking: None,
            timestamp: chrono::Utc::now(),
            response_items: Vec::new(),
        });

        // Simulate cancellation (same as denial)
        if let Some(last) = state.messages.last_mut() {
            if let Some(ref mut tcs) = last.tool_calls {
                for tc in tcs.iter_mut() {
                    if tc.id == "c1" {
                        tc.output = Some("Denied".into());
                        tc.status = ToolStatus::Error;
                        break;
                    }
                }
            }
        }

        let tc = &state.messages.last().unwrap().tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.output.as_deref(), Some("Denied"));
        assert_eq!(tc.status, ToolStatus::Error);
    }

    #[test]
    fn m2_progress_tracking_through_resolve() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![
            pa("a", "c1"),
            pa("b", "c2"),
            pa("c", "c3"),
            pa("d", "c4"),
        ]);
        assert_eq!(q.progress(), (0, 4));

        q.resolve();
        assert_eq!(q.progress(), (1, 4));

        q.resolve();
        assert_eq!(q.progress(), (2, 4));

        q.resolve();
        assert_eq!(q.progress(), (3, 4));

        let action = q.resolve();
        assert_eq!(q.progress(), (4, 4));
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn m2_in_flight_flag_blocks_duplicate_decisions() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);

        // User approves → mark in_flight
        q.mark_in_flight();
        assert!(q.is_in_flight());

        // While in_flight, is_active should still be true
        assert!(q.is_active());

        // Clear and resolve
        q.clear_in_flight();
        assert!(!q.is_in_flight());
        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
    }

    #[test]
    fn m2_single_item_batch_continue() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("only", "c1")]);
        assert!(q.is_active());
        // Single item batch: it's in current, nothing remains
        assert_eq!(q.progress(), (0, 1));

        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));
        assert!(!q.is_active());
    }

    // ─── M3: Queue invariant and cancellation tests ────────────────

    #[test]
    fn m3_resolve_idle_when_no_current() {
        let mut q = ApprovalQueue::new();
        // Empty queue: resolve should be Idle, not change counters
        let action = q.resolve();
        assert_eq!(action, ResolutionAction::Idle);
        assert_eq!(q.progress(), (0, 0));
    }

    #[test]
    fn m3_repeated_resolve_never_exceeds_total() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);

        // Resolve both items
        q.resolve(); // 1/2
        q.resolve(); // 2/2 → Continue

        // Repeated resolves after exhaustion must be Idle
        for _ in 0..5 {
            let action = q.resolve();
            assert_eq!(action, ResolutionAction::Idle);
            assert_eq!(q.progress(), (2, 2));
        }
        assert!(!q.is_active());
    }

    #[test]
    fn m3_exactly_one_continue_across_repeated_resolves() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);

        let action = q.resolve();
        assert!(matches!(action, ResolutionAction::Continue));

        // Subsequent resolves must be Idle, never another Continue
        for _ in 0..3 {
            let action = q.resolve();
            assert_eq!(action, ResolutionAction::Idle);
        }
    }

    #[test]
    fn m3_mark_in_flight_fails_without_current() {
        let mut q = ApprovalQueue::new();
        assert!(!q.mark_in_flight());
    }

    #[test]
    fn m3_mark_in_flight_fails_when_already_in_flight() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);
        assert!(q.mark_in_flight());
        assert!(!q.mark_in_flight()); // already in flight
    }

    #[test]
    fn m3_complete_in_flight_matching_tool_id() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);
        q.mark_in_flight();

        let action = q.complete_in_flight("c1");
        assert!(matches!(action, ResolutionAction::Advance { next, .. } if next.tool_id == "c2"));
        assert_eq!(q.progress(), (1, 2));
        assert!(!q.is_in_flight());
    }

    #[test]
    fn m3_complete_in_flight_stale_tool_id() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);
        q.mark_in_flight();

        // Stale id → Idle, no change
        let action = q.complete_in_flight("wrong-id");
        assert_eq!(action, ResolutionAction::Idle);
        assert!(q.is_in_flight()); // still in flight
        assert_eq!(q.progress(), (0, 1));
    }

    #[test]
    fn m3_complete_in_flight_not_in_flight() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1")]);
        // Don't mark in_flight
        let action = q.complete_in_flight("c1");
        assert_eq!(action, ResolutionAction::Idle);
    }

    #[test]
    fn m3_cancel_current_returns_tool_id() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);
        q.mark_in_flight();

        let outcome = q.cancel_current();
        let outcome = outcome.unwrap();
        assert_eq!(outcome.cancelled_tool_id, "c1");
        assert_eq!(q.progress(), (1, 2));
        assert!(!q.is_in_flight());
        assert!(q.current().is_some()); // advanced to next
        assert_eq!(q.current().unwrap().tool_id, "c2");
        // Action is Advance with progress (1, 2)
        match &outcome.action {
            ResolutionAction::Advance {
                next,
                resolved,
                total,
            } => {
                assert_eq!(next.tool_id, "c2");
                assert_eq!(*resolved, 1);
                assert_eq!(*total, 2);
            }
            other => panic!("Expected Advance, got {:?}", other),
        }
    }

    #[test]
    fn m3_cancel_current_no_item() {
        let mut q = ApprovalQueue::new();
        let outcome = q.cancel_current();
        assert!(outcome.is_none());
    }

    #[test]
    fn m3_cancel_first_of_two_advances_to_second() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("first", "c1"), pa("second", "c2")]);

        let outcome = q.cancel_current().unwrap();
        assert_eq!(outcome.cancelled_tool_id, "c1");
        // Queue advanced to second item
        assert_eq!(q.current().unwrap().tool_id, "c2");
        assert_eq!(q.progress(), (1, 2));
        assert!(q.is_active());
        match &outcome.action {
            ResolutionAction::Advance {
                next,
                resolved,
                total,
            } => {
                assert_eq!(next.tool_id, "c2");
                assert_eq!(*resolved, 1);
                assert_eq!(*total, 2);
            }
            other => panic!("Expected Advance, got {:?}", other),
        }
    }

    #[test]
    fn m3_cancel_sole_item_returns_continue() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("only", "c1")]);

        let outcome = q.cancel_current().unwrap();
        assert_eq!(outcome.cancelled_tool_id, "c1");
        assert!(q.current().is_none());
        assert!(!q.is_active());
        assert_eq!(q.progress(), (1, 1));
        assert!(matches!(outcome.action, ResolutionAction::Continue));
    }

    #[test]
    fn m3_repeated_cancel_is_noop_and_counters_unchanged() {
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("only", "c1")]);

        // First cancel succeeds
        let outcome = q.cancel_current().unwrap();
        assert_eq!(outcome.cancelled_tool_id, "c1");
        assert_eq!(q.progress(), (1, 1));

        // Repeated cancel is a no-op
        let outcome2 = q.cancel_current();
        assert!(outcome2.is_none());
        assert_eq!(q.progress(), (1, 1)); // counters unchanged
        assert!(!q.is_active());
    }

    #[test]
    fn m3_cancel_waiting_item_no_in_flight() {
        // Cancel on a waiting (not in-flight) item should still work
        let mut q = ApprovalQueue::new();
        q.begin_batch(vec![pa("a", "c1"), pa("b", "c2")]);
        // Do NOT mark in_flight — item is waiting, not executing

        let outcome = q.cancel_current().unwrap();
        assert_eq!(outcome.cancelled_tool_id, "c1");
        assert_eq!(q.current().unwrap().tool_id, "c2");
        assert!(!q.is_in_flight());
    }

    #[test]
    fn m3_persistent_deny_is_error_status() {
        use crate::app::{ResolvedToolResult, ResolvedToolStatus};

        let r = ResolvedToolResult {
            tool_id: "c1".into(),
            output: "Denied by persistent approval rule".into(),
            status: ResolvedToolStatus::Error,
            image: None,
            memory_proposal: None,
            mutation: None,
            command_observation: None,
        };
        assert_eq!(r.status, ResolvedToolStatus::Error);
    }

    #[test]
    fn m3_persistent_allow_is_done_status() {
        use crate::app::{ResolvedToolResult, ResolvedToolStatus};

        let r = ResolvedToolResult {
            tool_id: "c1".into(),
            output: "auto-executed: read_file".into(),
            status: ResolvedToolStatus::Done,
            image: None,
            memory_proposal: None,
            mutation: None,
            command_observation: None,
        };
        assert_eq!(r.status, ResolvedToolStatus::Done);
    }

    // ─── Existing approval-rule tests ─────────────────────────────

    #[test]
    fn test_tool_rule_matching() {
        let allow_rule = ApprovalRule::AllowTool("read_file".into());
        let args = serde_json::json!({"path": "src/main.rs"});
        assert!(matches_rule("read_file", &args, "/tmp", &allow_rule));
        assert!(!matches_rule("write_file", &args, "/tmp", &allow_rule));

        let deny_rule = ApprovalRule::DenyTool("run_command".into());
        assert!(matches_rule(
            "run_command",
            &serde_json::json!({"command": "rm -rf /"}),
            "/tmp",
            &deny_rule
        ));
    }

    #[test]
    fn test_path_rule_matching() {
        let rule = ApprovalRule::AllowPath {
            tool: "read_file".into(),
            path_pattern: "/Users/example/project/**/*.rs".into(),
        };
        let args = serde_json::json!({"path": "src/main.rs"});
        assert!(matches_rule(
            "read_file",
            &args,
            "/Users/example/project/ca",
            &rule
        ));

        let args2 = serde_json::json!({"path": "/etc/passwd"});
        assert!(!matches_rule(
            "read_file",
            &args2,
            "/Users/example/project/ca",
            &rule
        ));
    }

    #[test]
    fn test_check_approval_priority() {
        let rules = vec![
            ApprovalRule::AllowTool("read_file".into()),
            ApprovalRule::DenyPath {
                tool: "read_file".into(),
                path_pattern: "/etc/**".into(),
            },
        ];
        // Allowed by tool rule
        let args = serde_json::json!({"path": "src/main.rs"});
        assert_eq!(
            check_approval("read_file", &args, "/tmp", &rules),
            Some(true)
        );

        // Denied by path rule (takes priority)
        let args2 = serde_json::json!({"path": "/etc/passwd"});
        assert_eq!(
            check_approval("read_file", &args2, "/tmp", &rules),
            Some(false)
        );

        // No matching rule
        let args3 = serde_json::json!({"path": "test.txt"});
        assert_eq!(check_approval("write_file", &args3, "/tmp", &rules), None);
    }

    #[test]
    fn broker_prepares_before_rules_and_allow_never_widens_roots() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let primary = std::fs::canonicalize(root.path()).unwrap();
        let workspace = crate::workspace::WorkspaceContext::from_primary(primary.clone(), primary);
        let rules = vec![ApprovalRule::AllowTool("read_file".into())];
        let broker = ApprovalBroker::new(&workspace, &rules, ApprovalOrigin::Agent("child".into()));
        let error = broker
            .prepare(
                "read_file",
                &serde_json::json!({"path":outside.path().join("secret")}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap_err();
        assert!(error.contains("outside workspace roots") || error.contains("does not exist"));

        std::fs::write(root.path().join("ok"), "ok").unwrap();
        let (prepared, decision) = broker
            .prepare(
                "read_file",
                &serde_json::json!({"path":"ok"}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
        assert_eq!(decision, BrokerDecision::AllowedByRule);
        assert!(prepared.arguments["path"].as_str().unwrap().starts_with(
            std::fs::canonicalize(root.path())
                .unwrap()
                .to_str()
                .unwrap()
        ));
        assert_eq!(broker.origin, ApprovalOrigin::Agent("child".into()));
    }

    #[test]
    fn persistent_denial_precedes_the_distinct_network_prompt() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let rules = vec![ApprovalRule::DenyTool("run_command".into())];
        let broker = ApprovalBroker::new(&workspace, &rules, ApprovalOrigin::Main);

        let (_, decision) = broker
            .prepare(
                "run_command",
                &serde_json::json!({"command":"true", "network":true}),
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();

        assert_eq!(decision, BrokerDecision::DeniedByRule);
    }

    #[test]
    fn combined_interactive_decision_includes_network_without_overriding_ask_or_deny() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let arguments = serde_json::json!({"command":"ssh bsd 'ls -la'"});
        let ask = crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "run_command".into(),
            },
        };
        let deny = ApprovalRule::DenyTool("run_command".into());

        let broker = ApprovalBroker::with_policy(&workspace, &[], &[], &[], ApprovalOrigin::Main);
        let (_, ordinary) = broker
            .prepare(
                "run_command",
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
            )
            .unwrap();
        assert_eq!(ordinary, BrokerDecision::NetworkPrompt);
        let (_, combined) = broker
            .prepare_with_combined_network_decision(
                "run_command",
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(combined, BrokerDecision::Prompt);

        let ask_broker = ApprovalBroker::with_policy(
            &workspace,
            &[],
            std::slice::from_ref(&ask),
            &[],
            ApprovalOrigin::Main,
        );
        let (_, asked) = ask_broker
            .prepare_with_combined_network_decision(
                "run_command",
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(asked, BrokerDecision::ExplicitPrompt);

        let deny_broker = ApprovalBroker::new(
            &workspace,
            std::slice::from_ref(&deny),
            ApprovalOrigin::Main,
        );
        let (_, denied) = deny_broker
            .prepare_with_combined_network_decision(
                "run_command",
                &arguments,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(denied, BrokerDecision::DeniedByRule);
    }

    #[test]
    fn rich_policy_and_explicit_ask_precedence_are_shared_by_dynamic_mcp_classification() {
        let workspace = crate::workspace::WorkspaceContext::discover("/tmp").unwrap();
        let ask = crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "mcp_demo".into(),
            },
        };
        let broker = ApprovalBroker::with_policy(
            &workspace,
            &[],
            std::slice::from_ref(&ask),
            &[],
            ApprovalOrigin::Mcp("demo".into()),
        );
        let (_, decision) = broker.prepare_dynamic(
            "mcp_demo",
            &serde_json::json!({}),
            crate::agent::registry::ToolOrigin::Mcp("demo".into()),
            false,
        );
        assert_eq!(decision, BrokerDecision::ExplicitPrompt);

        let grant = crate::permission::PermissionMatcher::Tool {
            tool: "mcp_demo".into(),
        };
        let asks = [ask];
        let grants = [grant];
        let broker = ApprovalBroker::with_policy(
            &workspace,
            &[],
            &asks,
            &grants,
            ApprovalOrigin::Mcp("demo".into()),
        );
        let (_, decision) = broker.prepare_dynamic(
            "mcp_demo",
            &serde_json::json!({}),
            crate::agent::registry::ToolOrigin::Mcp("demo".into()),
            false,
        );
        assert_eq!(decision, BrokerDecision::ExplicitPrompt);
    }

    #[test]
    fn workflow_authorization_is_host_owned_and_explicit_rules_remain_ceilings() {
        let root = tempfile::tempdir().unwrap();
        let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
        let repeated = serde_json::json!({"workflow_id":"seedbox_archive"});
        let first_run = serde_json::json!({
            "workflow_id":"seedbox_archive",
            "accepted_plan_digest":"a".repeat(64)
        });
        let broker = ApprovalBroker::with_policy(&workspace, &[], &[], &[], ApprovalOrigin::Main);
        let (_, repeated_decision) = broker
            .prepare_with_combined_network_decision(
                "run_automation_workflow",
                &repeated,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(repeated_decision, BrokerDecision::AllowedByRule);
        let (_, first_decision) = broker
            .prepare_with_combined_network_decision(
                "run_automation_workflow",
                &first_run,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(first_decision, BrokerDecision::Prompt);

        let ask = crate::permission::PermissionRule {
            decision: crate::permission::PermissionDecision::Ask,
            matcher: crate::permission::PermissionMatcher::Tool {
                tool: "run_automation_workflow".into(),
            },
        };
        let ask_rules = [ask];
        let asked =
            ApprovalBroker::with_policy(&workspace, &[], &ask_rules, &[], ApprovalOrigin::Main);
        let (_, decision) = asked
            .prepare_with_combined_network_decision(
                "run_automation_workflow",
                &repeated,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(decision, BrokerDecision::ExplicitPrompt);

        let deny = ApprovalRule::DenyTool("run_automation_workflow".into());
        let deny_rules = [deny];
        let denied = ApprovalBroker::new(&workspace, &deny_rules, ApprovalOrigin::Main);
        let (_, decision) = denied
            .prepare_with_combined_network_decision(
                "run_automation_workflow",
                &repeated,
                crate::agent::registry::ToolOrigin::BuiltIn,
                true,
            )
            .unwrap();
        assert_eq!(decision, BrokerDecision::DeniedByRule);
    }
}
