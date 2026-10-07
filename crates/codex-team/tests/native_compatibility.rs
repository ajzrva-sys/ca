//! Legacy facades expose the exact native contract types, not converted copies.
#[test]
fn legacy_and_native_contracts_have_identical_type_and_wire_identity() {
    let native: ca_automation::runtime::ExecutionState =
        codex_team::runtime::ExecutionState::Prepared;
    assert_eq!(native, ca_automation::runtime::ExecutionState::Prepared);
    assert_eq!(
        codex_team_protocol::MECHANISTIC_REQUEST_SCHEMA,
        "codex-team.mechanistic_request.v1"
    );
    assert_eq!(
        codex_team_protocol::WORKFLOW_EXECUTION_RECEIPT_SCHEMA,
        "codex-team.workflow_execution_receipt.v1"
    );
    let native_schema = ca_automation::mechanistic::mechanistic_request_json_schema();
    assert_eq!(
        native_schema,
        codex_team_protocol::mechanistic_request_json_schema()
    );
}
