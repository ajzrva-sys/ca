//! Cross-process coverage for the import/revision/verification foundations.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

fn binary() -> PathBuf {
    support::codex_team_binary()
}

fn temp_repo(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("codex-team-it-{name}-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    let output = run(&root, "git", &["init", "-q"]);
    assert!(output.status.success());
    root
}

fn run(root: &Path, program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Output {
    let program = program.as_ref();
    Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|error| panic!("run {program:?} {args:?}: {error}"))
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = run(root, "git", args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn run_team(root: &Path, args: &[&str]) -> Output {
    run(root, binary(), args)
}

fn parse_success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "codex-team failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must be one JSON receipt")
}

#[test]
fn digest_bound_import_is_idempotent_and_hygiene_checks_are_rust_owned() {
    let repo = temp_repo("ca-import-foundations");
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);

    let init = run_team(
        &repo,
        &[
            "init",
            "--repo",
            repo.to_str().unwrap(),
            "--mission",
            "typed import integration test",
            "--commit-mode",
            "manual",
            "--require-clean-start",
        ],
    );
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git(
        &repo,
        &[
            "add",
            "AGENTS.md",
            "NEXT_STEPS.md",
            "NEXT_STEPS_non-ai.md",
            "ROADMAP.md",
        ],
    );
    git(&repo, &["commit", "-qm", "scaffold"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);

    let input_path = repo.with_extension("task-import.json");
    let input = json!({
        "schema": "codex-team.task_import.v1",
        "source": "ca",
        "base_head": head,
        "task": {
            "task_id": "INT-148",
            "title": "Scoped role-run contract",
            "kind": "protocol",
            "objective": "Exercise the typed import boundary",
            "why_unblocks_progress": "Makes external governance explicit",
            "allowed_files": ["src/role_run.rs", "src/main.rs"],
            "read_only_files": ["src/headless.rs"],
            "forbidden_actions": ["No runtime configuration mutation"],
            "steps": ["Characterize", "Implement", "Validate"],
            "acceptance_criteria": ["Binary observable result"],
            "verification_commands": ["cargo test role_run --no-fail-fast"],
            "stop_conditions": ["Stop on workspace escape"],
            "discovery_budget": "Read only named files unless a stop condition applies",
            "estimated_minutes": 180
        }
    });
    fs::write(&input_path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();

    let preview = parse_success(run_team(
        &repo,
        &[
            "task-import",
            "--repo",
            repo.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(preview["schema"], "codex-team.task_import_preview.v1");
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["mutation_performed"], false);
    let digest = preview["import_digest"].as_str().unwrap();
    let revision = preview["repository_revision"].as_str().unwrap();

    let stale_revision = "0".repeat(64);
    let stale = run_team(
        &repo,
        &[
            "task-import",
            "--repo",
            repo.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--accept-digest",
            digest,
            "--expected-revision",
            &stale_revision,
            "--json",
        ],
    );
    assert!(!stale.status.success());
    let pre_apply_state: Value = serde_json::from_slice(
        &fs::read(repo.join(".orchestrator/state.json")).expect("read state"),
    )
    .unwrap();
    assert!(pre_apply_state["queue"].as_array().unwrap().is_empty());
    assert!(pre_apply_state["imported_tasks"]
        .as_object()
        .unwrap()
        .is_empty());
    assert!(!repo.join(".orchestrator/imports").exists());

    let applied = parse_success(run_team(
        &repo,
        &[
            "task-import",
            "--repo",
            repo.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--accept-digest",
            digest,
            "--expected-revision",
            revision,
            "--json",
        ],
    ));
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head);
    assert!(git(&repo, &["diff", "--cached", "--name-only"]).is_empty());
    assert!(git(&repo, &["status", "--porcelain"]).is_empty());

    let state: Value = serde_json::from_slice(
        &fs::read(repo.join(".orchestrator/state.json")).expect("read state"),
    )
    .unwrap();
    assert_eq!(state["queue"][0]["task_id"], "INT-148");
    assert_eq!(state["imported_tasks"]["INT-148"]["import_digest"], digest);
    let receipt_path = repo.join(applied["receipt_path"].as_str().unwrap());
    let receipt_before = fs::read(&receipt_path).unwrap();

    let second_preview = parse_success(run_team(
        &repo,
        &[
            "task-import",
            "--repo",
            repo.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--json",
        ],
    ));
    let repeated = parse_success(run_team(
        &repo,
        &[
            "task-import",
            "--repo",
            repo.to_str().unwrap(),
            "--input",
            input_path.to_str().unwrap(),
            "--accept-digest",
            second_preview["import_digest"].as_str().unwrap(),
            "--expected-revision",
            second_preview["repository_revision"].as_str().unwrap(),
            "--json",
        ],
    ));
    assert_eq!(repeated["outcome"], "already_applied");
    assert_eq!(fs::read(&receipt_path).unwrap(), receipt_before);

    for check in ["snapshot-hygiene", "protected-trees"] {
        let receipt = parse_success(run_team(
            &repo,
            &["verify", check, "--repo", repo.to_str().unwrap(), "--json"],
        ));
        assert_eq!(receipt["schema"], "codex-team.verify.v1");
        assert_eq!(receipt["passed"], true);
    }

    fs::create_dir_all(repo.join("tests/snapshots")).unwrap();
    fs::write(repo.join("tests/snapshots/view.snap.new"), "pending").unwrap();
    let failed = run_team(
        &repo,
        &[
            "verify",
            "snapshot-hygiene",
            "--repo",
            repo.to_str().unwrap(),
            "--json",
        ],
    );
    assert!(!failed.status.success());
    let failed_receipt: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(failed_receipt["passed"], false);
    assert!(failed_receipt["paths"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path == "tests/snapshots/view.snap.new"));

    let _ = fs::remove_file(&input_path);
    let _ = fs::remove_dir_all(&repo);
}
