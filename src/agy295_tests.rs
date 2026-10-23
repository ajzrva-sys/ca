use super::*;

#[test]
fn agy295_permissions_why_opens_captured_decisions_without_changing_draft() {
    let mut state = tui::test_support::test_state();
    let config = CaConfig::default();
    state.input = "unfinished draft".into();
    state.input_cursor = 3;
    open_permission_manager(&mut state, &config, "run_command".into());
    assert!(state.permission_manager.is_some());
    open_permission_manager(&mut state, &config, "why".into());
    assert!(state.permission_manager.is_none());
    assert!(state.pager_open);
    assert!(state
        .pager_content
        .contains("Permission decision explanations"));
    assert_eq!(state.input, "unfinished draft");
    assert_eq!(state.input_cursor, 3);
}

#[test]
fn agy295_permission_explanation_pager_renders_narrow_and_wide() {
    let mut state = tui::test_support::test_state();
    state.pager_open = true;
    state.pager_content = approval::render_explanations(&approval::DecisionSnapshot {
        receipts: vec![approval::DecisionReceipt {
            sequence: 1,
            tool: "run_command".into(),
            caller: approval::DecisionCaller::Interactive,
            approval_origin: "main",
            tool_origin: "built-in",
            policy: permission::PermissionEvaluation {
                outcome: permission::PermissionOutcome::Ask,
                source: permission::PolicySource::PermissionRule,
                index: Some(0),
                matcher: Some(permission::MatcherCategory::Tool),
            },
            decision: approval::BrokerDecision::ExplicitPrompt,
            reason: approval::BrokerReason::ExplicitAsk,
            network_requested: false,
            network_combined: true,
        }],
        skipped: 0,
    });
    for (width, height) in [(60, 28), (100, 32)] {
        let screen = tui::test_support::render_to_text(&state, width, height);
        assert!(
            screen.contains("Permission decision explanations"),
            "{screen}"
        );
        assert!(screen.contains("run_command"), "{screen}");
        assert!(screen.contains("permission rule"), "{screen}");
        assert!(screen.contains("ExplicitPrompt"), "{screen}");
    }
}
