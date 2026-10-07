use anyhow::{bail, Result};
use rmcp::{
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod support;

fn temp_git_repo(name: &str) -> Result<PathBuf> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let repo = std::env::temp_dir().join(format!("codex-team-mcp-sdk-{name}-{nonce}"));
    fs::create_dir_all(&repo)?;
    run_git(&repo, &["init", "--quiet"])?;
    Ok(fs::canonicalize(repo)?)
}

fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        bail!("git {:?} failed", args);
    }
    Ok(())
}

fn child_transport(repo: &Path, access: &str) -> Result<TokioChildProcess> {
    let binary = support::codex_team_binary();
    let broker_root = broker_test_root(repo);
    fs::create_dir_all(&broker_root)?;
    Ok(TokioChildProcess::new(
        tokio::process::Command::new(binary).configure(|command| {
            command.env("CODEX_TEAM_TEST_ISOLATION_READY", "1");
            command.env("CODEX_TEAM_TEST_BROKER_ROOT", &broker_root);
            command.env("CODEX_TEAM_TEST_BROKER_IDLE_SECONDS", "1");
            command.args([
                "mcp",
                "--repo",
                repo.to_str().expect("temporary path is UTF-8"),
                "--access",
                access,
            ]);
        }),
    )?)
}

fn run_codex_team(repo: &Path, args: &[&str]) -> Result<()> {
    let output = StdCommand::new(support::codex_team_binary())
        .args(args)
        .current_dir(repo)
        .output()?;
    if !output.status.success() {
        bail!(
            "codex-team {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn stop_test_broker(repo: &Path) -> Result<()> {
    let output = StdCommand::new(support::codex_team_binary())
        .args(["broker", "stop", "--repo"])
        .arg(repo)
        .arg("--json")
        .env("CODEX_TEAM_TEST_BROKER_ROOT", broker_test_root(repo))
        .output()?;
    if !output.status.success() {
        bail!(
            "broker stop failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn test_broker_status(repo: &Path) -> Result<Value> {
    let output = StdCommand::new(support::codex_team_binary())
        .args(["broker", "status", "--repo"])
        .arg(repo)
        .arg("--json")
        .env("CODEX_TEAM_TEST_BROKER_ROOT", broker_test_root(repo))
        .output()?;
    if !output.status.success() {
        bail!(
            "broker status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn broker_test_root(repo: &Path) -> PathBuf {
    let digest = format!("{:x}", Sha256::digest(repo.as_os_str().as_encoded_bytes()));
    PathBuf::from("/tmp").join(format!("cb-{}", &digest[..12]))
}

async fn wait_for_barrier(path: &Path) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if path.is_file() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for test barrier {}", path.display());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn real_stdio_observe_negotiates_exact_tools_and_zero_resources() -> Result<()> {
    let repo = temp_git_repo("observe")?;
    let client = ().serve(child_transport(&repo, "observe")?).await?;
    let tools = client.list_tools(None).await?;
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "status",
            "doctor",
            "repo_query",
            "recipe_list",
            "recipe_preview",
            "self_improve_status",
            "self_improve_objectives",
            "self_improve_evaluations",
            "self_improve_traces",
            "self_improve_deployments",
            "step_status"
        ]
    );
    assert!(tools.tools.iter().all(|tool| {
        tool.input_schema.get("additionalProperties") == Some(&Value::Bool(false))
    }));
    let resources = client.list_resources(None).await?;
    assert!(resources.resources.is_empty());

    let status = client
        .call_tool(CallToolRequestParams::new("status"))
        .await?;
    assert_eq!(status.is_error, Some(false));
    let status = status.structured_content.as_ref().unwrap();
    assert_eq!(status["initialized"], false);
    assert_eq!(status["orchestration_available"], false);
    assert_eq!(status["revision"].as_str().unwrap().len(), 64);

    let doctor = client
        .call_tool(CallToolRequestParams::new("doctor"))
        .await?;
    assert_eq!(doctor.is_error, Some(true));
    assert_eq!(
        doctor.structured_content.as_ref().unwrap()["initialized"],
        false
    );

    let missing = client
        .call_tool(
            CallToolRequestParams::new("step_status").with_arguments(
                json!({"job_id": "missing", "wait_seconds": 0})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(
        missing.structured_content.as_ref().unwrap()["error"]["code"],
        "JOB_STATUS_FAILED"
    );
    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn real_stdio_delegate_is_observe_plus_durable_delegation() -> Result<()> {
    let repo = temp_git_repo("delegate")?;
    let client = ().serve(child_transport(&repo, "delegate")?).await?;
    let tools = client.list_tools(None).await?;
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for name in [
        "repo_query",
        "delegation_start",
        "delegation_status",
        "delegation_cancel",
        "delegation_feedback",
    ] {
        assert!(names.contains(&name), "missing delegate tool {name}");
    }
    for forbidden in ["step_start", "task_import_apply", "recipe_approve"] {
        assert!(!names.contains(&forbidden), "delegate exposed {forbidden}");
    }
    assert!(tools.tools.iter().all(|tool| {
        tool.input_schema.get("additionalProperties") == Some(&Value::Bool(false))
    }));
    let resources = client.list_resources(None).await?;
    assert!(resources.resources.is_empty());
    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn five_stdio_clients_share_one_query_and_persistent_receipt() -> Result<()> {
    let repo = temp_git_repo("shared-broker-query")?;
    fs::write(repo.join("tracked.txt"), "shared broker evidence\n")?;
    run_git(&repo, &["add", "tracked.txt"])?;
    run_git(
        &repo,
        &[
            "-c",
            "user.name=codex-team-test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;

    let client1 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client2 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client3 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client4 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client5 = ().serve(child_transport(&repo, "delegate")?).await?;
    let status = client1
        .call_tool(CallToolRequestParams::new("status"))
        .await?;
    let revision = status.structured_content.as_ref().unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_string();
    let arguments = json!({
        "schema": "codex-team.mechanistic_request.v1",
        "expected_revision": revision,
        "delegation_policy": "prefer",
        "allow_unisolated_evidence": false,
        "actions": [{
            "kind": "read_tracked_file",
            "path": "tracked.txt",
            "start_line": 1,
            "max_lines": 10
        }]
    })
    .as_object()
    .unwrap()
    .clone();
    let (one, two, three, four, five) = tokio::join!(
        client1
            .call_tool(CallToolRequestParams::new("repo_query").with_arguments(arguments.clone())),
        client2
            .call_tool(CallToolRequestParams::new("repo_query").with_arguments(arguments.clone())),
        client3
            .call_tool(CallToolRequestParams::new("repo_query").with_arguments(arguments.clone())),
        client4
            .call_tool(CallToolRequestParams::new("repo_query").with_arguments(arguments.clone())),
        client5
            .call_tool(CallToolRequestParams::new("repo_query").with_arguments(arguments.clone())),
    );
    let receipts = [one?, two?, three?, four?, five?]
        .into_iter()
        .map(|result| result.structured_content.unwrap())
        .collect::<Vec<_>>();
    assert!(
        receipts
            .iter()
            .all(|receipt| receipt["status"] == "completed"),
        "shared query receipts: {receipts:#?}"
    );
    let source_digests = receipts
        .iter()
        .filter_map(|receipt| receipt["broker"]["source_receipt_digest"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(source_digests.len(), 1);
    assert_eq!(
        receipts
            .iter()
            .filter(|receipt| receipt["broker"]["disposition"] == "executed")
            .count(),
        1
    );

    let overlap_a = json!({
        "schema": "codex-team.mechanistic_request.v1",
        "expected_revision": revision,
        "delegation_policy": "prefer",
        "actions": [
            {"kind":"read_tracked_file","path":"tracked.txt","start_line":1,"max_lines":10},
            {"kind":"git_history","max_entries":4}
        ]
    })
    .as_object()
    .unwrap()
    .clone();
    let overlap_b = json!({
        "schema": "codex-team.mechanistic_request.v1",
        "expected_revision": revision,
        "delegation_policy": "prefer",
        "actions": [
            {"kind":"git_history","max_entries":4},
            {"kind":"list_tracked_files","prefix":null,"max_entries":20}
        ]
    })
    .as_object()
    .unwrap()
    .clone();
    let (overlap_a, overlap_b) = tokio::join!(
        client1.call_tool(CallToolRequestParams::new("repo_query").with_arguments(overlap_a)),
        client2.call_tool(CallToolRequestParams::new("repo_query").with_arguments(overlap_b))
    );
    let overlap_a = overlap_a?.structured_content.unwrap();
    let overlap_b = overlap_b?.structured_content.unwrap();
    let overlap_a_results = overlap_a["action_results"]
        .as_array()
        .unwrap_or_else(|| panic!("overlapping query A did not return results: {overlap_a:#}"));
    assert_eq!(
        overlap_a_results
            .iter()
            .map(|result| result["action_index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let overlap_b_results = overlap_b["action_results"]
        .as_array()
        .unwrap_or_else(|| panic!("overlapping query B did not return results: {overlap_b:#}"));
    assert_eq!(
        overlap_b_results
            .iter()
            .map(|result| result["action_index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        overlap_a["action_results"][1]["entries"],
        overlap_b["action_results"][0]["entries"]
    );

    let later = ().serve(child_transport(&repo, "delegate")?).await?;
    let mut later_arguments = arguments;
    later_arguments.insert(
        "purpose".to_string(),
        Value::String("automatic_preflight".to_string()),
    );
    let cached = later
        .call_tool(CallToolRequestParams::new("repo_query").with_arguments(later_arguments))
        .await?;
    assert_eq!(
        cached.structured_content.as_ref().unwrap()["broker"]["disposition"],
        "cache_hit"
    );
    assert_eq!(test_broker_status(&repo)?["running"], true);
    let (later, client1, client2, client3, client4, client5) = tokio::join!(
        later.cancel(),
        client1.cancel(),
        client2.cancel(),
        client3.cancel(),
        client4.cancel(),
        client5.cancel(),
    );
    later?;
    client1?;
    client2?;
    client3?;
    client4?;
    client5?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert_eq!(test_broker_status(&repo)?["running"], false);
    stop_test_broker(&repo)?;
    fs::remove_dir_all(broker_test_root(&repo))?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn five_stdio_clients_share_evidence_cache_and_last_consumer_cancellation() -> Result<()> {
    let repo = temp_git_repo("shared-broker-evidence")?;
    fs::create_dir_all(repo.join("src"))?;
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"broker-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )?;
    fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n")?;
    fs::write(repo.join(".gitignore"), ".codex-team-test-build-*\n")?;
    fs::write(
        repo.join("build.rs"),
        r#"use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

fn marker(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join(name)
}

fn main() {
    let waiting = marker(".codex-team-test-build-waiting");
    let release = marker(".codex-team-test-build-release");
    let released = marker(".codex-team-test-build-released");
    let finish = marker(".codex-team-test-build-finish");
    fs::write(waiting, b"waiting\n").unwrap();
    while !release.is_file() {
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(released, b"released\n").unwrap();
    while !finish.is_file() {
        thread::sleep(Duration::from_millis(10));
    }
}
"#,
    )?;
    let lock = StdCommand::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(&repo)
        .output()?;
    if !lock.status.success() {
        bail!(
            "generate fixture lockfile failed: {}",
            String::from_utf8_lossy(&lock.stderr)
        );
    }
    run_git(
        &repo,
        &[
            "add",
            ".gitignore",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "src/lib.rs",
        ],
    )?;
    run_git(
        &repo,
        &[
            "-c",
            "user.name=codex-team-test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;

    let client1 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client2 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client3 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client4 = ().serve(child_transport(&repo, "delegate")?).await?;
    let client5 = ().serve(child_transport(&repo, "delegate")?).await?;
    let status = client1
        .call_tool(CallToolRequestParams::new("status"))
        .await?;
    let revision = status.structured_content.as_ref().unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_string();
    let evidence_arguments = |recipe: &str| {
        json!({
            "schema": "codex-team.mechanistic_request.v1",
            "expected_revision": revision,
            "delegation_policy": "prefer",
            "purpose": "automatic_verification",
            "allow_unisolated_evidence": false,
            "actions": [{
                "kind": "evidence",
                "recipe": { "kind": recipe },
                "timeout_seconds": 30
            }]
        })
        .as_object()
        .unwrap()
        .clone()
    };
    let barrier_waiting = repo.join(".codex-team-test-build-waiting");
    let barrier_release = repo.join(".codex-team-test-build-release");
    let barrier_released = repo.join(".codex-team-test-build-released");
    let barrier_finish = repo.join(".codex-team-test-build-finish");

    let check = evidence_arguments("cargo_check");
    let (one, two, three, four, five) = tokio::join!(
        client1.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(check.clone())
        ),
        client2.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(check.clone())
        ),
        client3.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(check.clone())
        ),
        client4.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(check.clone())
        ),
        client5.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(check.clone())
        ),
    );
    let starts = [one?, two?, three?, four?, five?]
        .into_iter()
        .map(|result| result.structured_content.unwrap())
        .collect::<Vec<_>>();
    let job_ids = starts
        .iter()
        .filter_map(|value| value["job_id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(job_ids.len(), 1, "shared evidence starts: {starts:#?}");
    assert_eq!(
        starts
            .iter()
            .filter(|value| value["broker"]["disposition"] == "executed")
            .count(),
        1
    );
    assert_eq!(
        starts
            .iter()
            .filter(|value| value["broker"]["disposition"] == "attached")
            .count(),
        4
    );
    let job_id = job_ids.into_iter().next().unwrap().to_string();
    wait_for_barrier(&barrier_waiting).await?;
    let registered = client1
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": job_id, "wait_seconds": 0})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    let registered = registered.structured_content.unwrap();
    assert_eq!(registered["status"], "running", "{registered:#?}");
    assert_eq!(registered["broker"]["consumer_count"], 5, "{registered:#?}");
    fs::write(&barrier_release, b"release\n")?;
    fs::write(&barrier_finish, b"finish\n")?;
    wait_for_barrier(&barrier_released).await?;
    let completed = client1
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": job_id, "wait_seconds": 20})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    let completed = completed.structured_content.unwrap();
    assert_eq!(completed["status"], "completed", "{completed:#?}");
    let source_digest = completed["mechanistic_receipt"]["broker"]["source_receipt_digest"]
        .as_str()
        .unwrap()
        .to_string();

    let later = ().serve(child_transport(&repo, "delegate")?).await?;
    let mut ordinary_check = check.clone();
    ordinary_check.remove("purpose");
    let cached = later
        .call_tool(CallToolRequestParams::new("delegation_start").with_arguments(ordinary_check))
        .await?;
    let cached = cached.structured_content.unwrap();
    assert_eq!(cached["broker"]["disposition"], "cache_hit");
    let cached_status = later
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": cached["job_id"], "wait_seconds": 0})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        cached_status.structured_content.as_ref().unwrap()["mechanistic_receipt"]["broker"]
            ["source_receipt_digest"],
        source_digest
    );
    assert_eq!(
        cached_status.structured_content.as_ref().unwrap()["mechanistic_receipt"]["request_digest"],
        cached["request_digest"]
    );

    for marker in [
        &barrier_waiting,
        &barrier_release,
        &barrier_released,
        &barrier_finish,
    ] {
        match fs::remove_file(marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    let build = evidence_arguments("cargo_build");
    let (one, two, three, four, five) = tokio::join!(
        client1.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(build.clone())
        ),
        client2.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(build.clone())
        ),
        client3.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(build.clone())
        ),
        client4.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(build.clone())
        ),
        client5.call_tool(
            CallToolRequestParams::new("delegation_start").with_arguments(build.clone())
        ),
    );
    let cancellation_starts = [one?, two?, three?, four?, five?]
        .into_iter()
        .map(|result| result.structured_content.unwrap())
        .collect::<Vec<_>>();
    let cancel_job = cancellation_starts[0]["job_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        cancellation_starts
            .iter()
            .all(|value| value["job_id"] == cancel_job),
        "shared cancellation starts: {cancellation_starts:#?}"
    );
    wait_for_barrier(&barrier_waiting).await?;
    let registered = client1
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": cancel_job, "wait_seconds": 0})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    let registered = registered.structured_content.unwrap();
    assert_eq!(registered["status"], "running", "{registered:#?}");
    assert_eq!(registered["broker"]["consumer_count"], 5, "{registered:#?}");
    fs::write(&barrier_release, b"release\n")?;
    wait_for_barrier(&barrier_released).await?;
    let busy_stop = StdCommand::new(support::codex_team_binary())
        .args(["broker", "stop", "--repo"])
        .arg(&repo)
        .arg("--json")
        .env("CODEX_TEAM_TEST_BROKER_ROOT", broker_test_root(&repo))
        .output()?;
    assert!(!busy_stop.status.success());
    assert!(String::from_utf8_lossy(&busy_stop.stderr).contains("jobs are active"));
    let cancel_args = json!({"job_id": cancel_job}).as_object().unwrap().clone();
    for client in [&client2, &client3, &client4, &client5] {
        let detached = client
            .call_tool(
                CallToolRequestParams::new("delegation_cancel").with_arguments(cancel_args.clone()),
            )
            .await?;
        assert_eq!(
            detached.structured_content.as_ref().unwrap()["status"],
            "detached"
        );
    }
    let still_running = client1
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": cancel_job, "wait_seconds": 0})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        still_running.structured_content.as_ref().unwrap()["status"],
        "running"
    );
    let final_cancel = client1
        .call_tool(CallToolRequestParams::new("delegation_cancel").with_arguments(cancel_args))
        .await?;
    assert_eq!(
        final_cancel.structured_content.as_ref().unwrap()["status"],
        "cancellation_requested"
    );
    let cancelled = client1
        .call_tool(
            CallToolRequestParams::new("delegation_status").with_arguments(
                json!({"job_id": cancel_job, "wait_seconds": 20})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        cancelled.structured_content.as_ref().unwrap()["status"],
        "cancelled"
    );

    later.cancel().await?;
    client1.cancel().await?;
    client2.cancel().await?;
    client3.cancel().await?;
    client4.cancel().await?;
    client5.cancel().await?;
    stop_test_broker(&repo)?;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    fs::remove_dir_all(broker_test_root(&repo))?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn real_stdio_operate_is_explicit_and_closed_schema() -> Result<()> {
    let repo = temp_git_repo("operate")?;
    fs::create_dir_all(repo.join(".orchestrator"))?;
    fs::write(
        repo.join(".orchestrator/config.toml"),
        "commit_mode = \"manual\"\nrequire_clean_start = true\n",
    )?;
    fs::write(
        repo.join(".orchestrator/state.json"),
        "{\"mission\":\"test\"}\n",
    )?;

    let client = ().serve(child_transport(&repo, "operate")?).await?;
    let tools = client.list_tools(None).await?;
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<Vec<_>>();
    for name in [
        "status",
        "doctor",
        "repo_query",
        "step_status",
        "recipe_list",
        "recipe_preview",
        "recipe_approve",
        "recipe_reject",
        "recipe_reactivate",
        "recipe_retire",
        "delegation_start",
        "delegation_status",
        "delegation_cancel",
        "delegation_feedback",
        "task_import_preview",
        "task_import_apply",
        "step_start",
        "step_cancel",
        "commit_pending",
        "disposition",
    ] {
        assert!(names.contains(&name), "missing operate tool {name}");
    }
    assert!(tools.tools.iter().all(|tool| {
        tool.input_schema.get("additionalProperties") == Some(&Value::Bool(false))
    }));
    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn real_stdio_disposition_is_stale_guarded_without_state_mutation() -> Result<()> {
    let repo = temp_git_repo("disposition")?;
    fs::create_dir_all(repo.join(".orchestrator"))?;
    fs::write(
        repo.join(".orchestrator/config.toml"),
        "commit_mode = \"manual\"\nrequire_clean_start = true\n",
    )?;
    fs::write(
        repo.join(".orchestrator/state.json"),
        serde_json::to_vec_pretty(&json!({
            "mission": "MCP guarded disposition",
            "human_disposition_required": {
                "reason": "A human must choose the next safe action",
                "options": ["preserve", "recheck", "gate", "keep-paused"],
                "source": "mcp-test"
            }
        }))?,
    )?;

    let client = ().serve(child_transport(&repo, "operate")?).await?;
    let status = client
        .call_tool(CallToolRequestParams::new("status"))
        .await?;
    assert_eq!(status.is_error, Some(false));
    let status = status.structured_content.as_ref().unwrap();
    let revision = status["revision"].as_str().unwrap();
    let pause_fingerprint = status["pause_fingerprint"].as_str().unwrap();
    let state_path = repo.join(".orchestrator/state.json");
    let state_before = fs::read(&state_path)?;

    let stale_revision = client
        .call_tool(
            CallToolRequestParams::new("disposition").with_arguments(
                json!({
                    "decision": "keep-paused",
                    "expected_revision": "0".repeat(64),
                    "pause_fingerprint": pause_fingerprint
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(stale_revision.is_error, Some(true));
    assert_eq!(fs::read(&state_path)?, state_before);

    let stale_pause = client
        .call_tool(
            CallToolRequestParams::new("disposition").with_arguments(
                json!({
                    "decision": "keep-paused",
                    "expected_revision": revision,
                    "pause_fingerprint": "0".repeat(64)
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(stale_pause.is_error, Some(true));
    assert_eq!(fs::read(&state_path)?, state_before);

    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn real_stdio_async_no_work_step_is_credentialless_and_noncommitting() -> Result<()> {
    let repo = temp_git_repo("no-work")?;
    run_git(&repo, &["config", "user.email", "mcp-test@example.invalid"])?;
    run_git(&repo, &["config", "user.name", "MCP Test"])?;
    run_codex_team(
        &repo,
        &[
            "init",
            "--repo",
            repo.to_str().unwrap(),
            "--mission",
            "MCP no-work compatibility test",
            "--commit-mode",
            "manual",
            "--require-clean-start",
        ],
    )?;
    run_git(
        &repo,
        &[
            "add",
            "ROADMAP.md",
            "NEXT_STEPS.md",
            "NEXT_STEPS_non-ai.md",
            "AGENTS.md",
        ],
    )?;
    run_git(
        &repo,
        &["commit", "--quiet", "-m", "initialize test target"],
    )?;

    let state_path = repo.join(".orchestrator/state.json");
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    state["project_complete"] = Value::Bool(true);
    state["master_due"] = Value::Bool(false);
    state["planner_due"] = Value::Bool(false);
    state["needs_reconciliation"] = Value::Bool(false);
    fs::write(&state_path, serde_json::to_vec_pretty(&state)?)?;

    let head_before = String::from_utf8(
        StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()?
            .stdout,
    )?;
    let client = ().serve(child_transport(&repo, "operate")?).await?;
    let status = client
        .call_tool(CallToolRequestParams::new("status"))
        .await?;
    assert_eq!(status.is_error, Some(false));
    let revision = status.structured_content.as_ref().unwrap()["revision"]
        .as_str()
        .expect("status has revision");
    let started = client
        .call_tool(
            CallToolRequestParams::new("step_start").with_arguments(
                json!({"expected_revision": revision})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(started.is_error, Some(false));
    let job_id = started.structured_content.as_ref().unwrap()["job_id"]
        .as_str()
        .unwrap();
    let terminal = client
        .call_tool(
            CallToolRequestParams::new("step_status").with_arguments(
                json!({"job_id": job_id, "wait_seconds": 10})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(terminal.is_error, Some(false));
    let terminal = terminal.structured_content.as_ref().unwrap();
    assert_eq!(terminal["status"], "completed");
    assert_eq!(terminal["cycle_receipt"]["outcome"], "no_work");
    assert_eq!(terminal["repository_may_be_partial"], false);

    let head_after = String::from_utf8(
        StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()?
            .stdout,
    )?;
    assert_eq!(head_after, head_before);
    assert!(StdCommand::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(&repo)
        .status()?
        .success());
    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}

#[tokio::test]
async fn real_stdio_task_import_is_previewed_stale_guarded_and_applied() -> Result<()> {
    let repo = temp_git_repo("task-import")?;
    run_git(&repo, &["config", "user.email", "mcp-test@example.invalid"])?;
    run_git(&repo, &["config", "user.name", "MCP Test"])?;
    run_codex_team(
        &repo,
        &[
            "init",
            "--repo",
            repo.to_str().unwrap(),
            "--mission",
            "MCP task import compatibility test",
            "--commit-mode",
            "manual",
            "--require-clean-start",
        ],
    )?;
    run_git(
        &repo,
        &[
            "add",
            "ROADMAP.md",
            "NEXT_STEPS.md",
            "NEXT_STEPS_non-ai.md",
            "AGENTS.md",
        ],
    )?;
    run_git(
        &repo,
        &["commit", "--quiet", "-m", "initialize test target"],
    )?;
    let head_before = String::from_utf8(
        StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()?
            .stdout,
    )?;
    let head = head_before.trim();
    let task = json!({
        "schema": "codex-team.task_import.v1",
        "source": "ca",
        "base_head": head,
        "task": {
            "task_id": "INT-148",
            "title": "MCP typed import",
            "kind": "protocol",
            "objective": "Prove digest-bound MCP task adoption",
            "why_unblocks_progress": "It establishes the supervised import boundary",
            "allowed_files": ["src/role_run.rs"],
            "read_only_files": ["src/headless.rs"],
            "forbidden_actions": ["No runtime configuration mutation"],
            "steps": ["Characterize", "Implement", "Validate"],
            "acceptance_criteria": ["Typed import is present in state"],
            "verification_commands": ["cargo fmt --all --check"],
            "stop_conditions": ["Stop on workspace escape"],
            "discovery_budget": "Read only named files",
            "estimated_minutes": 180
        }
    });

    let client = ().serve(child_transport(&repo, "operate")?).await?;
    let preview = client
        .call_tool(
            CallToolRequestParams::new("task_import_preview")
                .with_arguments(task.as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(preview.is_error, Some(false));
    let preview = preview.structured_content.as_ref().unwrap();
    assert_eq!(preview["valid"], true);
    let digest = preview["import_digest"].as_str().unwrap();
    let revision = preview["repository_revision"].as_str().unwrap();
    let state_before = fs::read(repo.join(".orchestrator/state.json"))?;

    let stale = client
        .call_tool(
            CallToolRequestParams::new("task_import_apply").with_arguments(
                json!({
                    "task": task,
                    "preview_digest": digest,
                    "expected_revision": "0".repeat(64)
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(stale.is_error, Some(true));
    assert_eq!(
        fs::read(repo.join(".orchestrator/state.json"))?,
        state_before
    );

    let applied = client
        .call_tool(
            CallToolRequestParams::new("task_import_apply").with_arguments(
                json!({
                    "task": task,
                    "preview_digest": digest,
                    "expected_revision": revision
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(applied.is_error, Some(false));
    assert_eq!(
        applied.structured_content.as_ref().unwrap()["outcome"],
        "applied"
    );
    let state_after: Value =
        serde_json::from_slice(&fs::read(repo.join(".orchestrator/state.json"))?)?;
    assert_eq!(state_after["queue"][0]["task_id"], "INT-148");
    let head_after = String::from_utf8(
        StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()?
            .stdout,
    )?;
    assert_eq!(head_after, head_before);
    assert!(StdCommand::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(&repo)
        .status()?
        .success());
    client.cancel().await?;
    fs::remove_dir_all(repo)?;
    Ok(())
}
