use ca_automation::native::{AutomationEngine, EngineOperation};
use ca_automation_protocol::{
    DelegationPolicy, MechanisticActionV1, MechanisticRequestPurposeV1, MechanisticRequestV1,
    MECHANISTIC_REQUEST_SCHEMA,
};
use std::path::Path;
use std::process::{Command, Output};

fn command(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ca"))
        .env("HOME", home)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .current_dir(cwd)
        .arg("automation-engine")
        .args(args)
        .output()
        .unwrap()
}

fn success(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn built_cli_query_has_durable_identity_without_provider_or_mcp() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("home");
    let repo = root.join("repository");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@localhost"]);
    std::fs::write(repo.join("input.txt"), "fixture\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "Fixture"]);
    let operation = EngineOperation::Mechanistic {
        repository: repo.clone(),
        request: MechanisticRequestV1 {
            schema: MECHANISTIC_REQUEST_SCHEMA.into(),
            expected_revision: AutomationEngine::repository_revision(&repo).unwrap(),
            delegation_policy: DelegationPolicy::Require,
            purpose: MechanisticRequestPurposeV1::Ordinary,
            allow_unisolated_evidence: false,
            actions: vec![MechanisticActionV1::GitHistory { max_entries: 3 }],
        },
    };
    let input = root.join("operation.json");
    std::fs::write(&input, serde_json::to_vec(&operation).unwrap()).unwrap();
    let input = input.to_str().unwrap();
    let run_id = "00000000-0000-4000-8000-000000000081";
    let preview = success(command(
        &home,
        &repo,
        &[
            "preview",
            input,
            "--run-id",
            run_id,
            "--sandbox",
            "policy-only",
        ],
    ));
    assert!(!home.exists(), "preview must leave ca storage untouched");
    let confirm = preview["confirmation_digest"].as_str().unwrap();
    let args = [
        "start",
        input,
        "--run-id",
        run_id,
        "--sandbox",
        "policy-only",
        "--confirm",
        confirm,
    ];
    let receipt = success(command(&home, &repo, &args));
    assert_eq!(receipt["run_id"], run_id);
    assert_eq!(receipt["state"], "succeeded");
    assert_eq!(
        receipt,
        success(command(&home, &repo, &args)),
        "duplicate dispatch must return the same receipt"
    );
    for action in ["status", "reconcile", "cancel"] {
        assert_eq!(receipt, success(command(&home, &repo, &[action, run_id])));
    }
    let foreign = command(&home, &repo, &["--profile", "another", "status", run_id]);
    assert!(!foreign.status.success());
    let second_id = "00000000-0000-4000-8000-000000000082";
    let preview = success(command(
        &home,
        &repo,
        &[
            "preview",
            input,
            "--run-id",
            second_id,
            "--sandbox",
            "policy-only",
        ],
    ));
    let second = success(command(
        &home,
        &repo,
        &[
            "start",
            input,
            "--run-id",
            second_id,
            "--sandbox",
            "policy-only",
            "--confirm",
            preview["confirmation_digest"].as_str().unwrap(),
        ],
    ));
    assert_eq!(second["state"], "succeeded");
    assert_eq!(second["run_id"], second_id);
    assert!(
        !home.join(".config/ca/config.toml").exists(),
        "typed query must not initialize a provider configuration"
    );
}

#[test]
fn built_cli_migration_previews_copies_exact_bytes_and_doctor_reports_missing_git() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    let home = root.join("home");
    let source = root.join("legacy");
    std::fs::create_dir_all(source.join("workflows/v1")).unwrap();
    let bytes = b"legacy approval bytes\n";
    std::fs::write(source.join("workflows/v1/receipt.json"), bytes).unwrap();
    let source_arg = source.to_str().unwrap();
    let preview = success(command(
        &home,
        &root,
        &["config-migration", "preview", "--source", source_arg],
    ));
    assert!(!home.exists());
    let digest = preview["migration_digest"]
        .as_str()
        .or_else(|| preview["digest"].as_str())
        .unwrap();
    let applied = success(command(
        &home,
        &root,
        &[
            "config-migration",
            "apply",
            "--source",
            source_arg,
            "--confirm",
            digest,
        ],
    ));
    assert_eq!(applied["installed_records"], 1);
    assert_eq!(
        std::fs::read(source.join("workflows/v1/receipt.json")).unwrap(),
        bytes
    );
    let doctor = success(command(
        &home,
        &root,
        &["doctor", "--repository", root.to_str().unwrap()],
    ));
    assert!(
        doctor.to_string().contains("Git"),
        "missing Git binding must be actionable: {doctor}"
    );
}
