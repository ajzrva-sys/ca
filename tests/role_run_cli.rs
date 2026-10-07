use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};

fn install_role_profile(
    home: &std::path::Path,
    base_url: &str,
    extra_policy: &str,
) -> std::path::PathBuf {
    let profile_dir = home.join(".config/ca/profiles");
    std::fs::create_dir_all(&profile_dir).unwrap();
    let path = profile_dir.join("codex-team.toml");
    std::fs::write(
        &path,
        format!(
            r#"approval_rules = []
{extra_policy}

[agent]
max_turns = 3
approval_mode = "allowlist"
command_timeout_secs = 2
sandbox_mode = "required"

[defaults]
provider = "fake"
model = "fake-model"

[providers.fake]
base_url = "{base_url}"
api_key = "${{ROLE_RUN_TEST_KEY}}"
models = ["fake-model"]

[model_profiles."fake/fake-model"]
supports_tools = true
reasoning_efforts = ["high"]
default_reasoning_effort = "high"
reasoning_request_format = "reasoning_effort"
"#
        ),
    )
    .unwrap();
    path
}

fn ca_command(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ca"));
    command
        .env("HOME", home)
        .env("ROLE_RUN_TEST_KEY", "ROLE-RUN-SECRET-CANARY");
    command
}

fn run_role(
    home: &std::path::Path,
    workspace: &std::path::Path,
    request: &serde_json::Value,
) -> Output {
    let mut child = ca_command(home)
        .current_dir(workspace)
        .args(["role-run", "--profile", "codex-team"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.as_mut().unwrap(), request).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn start_fake_provider(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    start_fake_provider_with_first_request_hook(responses, None)
}

fn start_fake_provider_with_first_request_hook(
    responses: Vec<String>,
    hook: Option<Box<dyn FnOnce() + Send>>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        let mut hook = hook;
        for body in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "provider request ended before headers");
                request.extend_from_slice(&chunk[..count]);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0, "provider request ended before body");
                request.extend_from_slice(&chunk[..count]);
            }
            requests.push(String::from_utf8_lossy(&request).into_owned());
            if let Some(hook) = hook.take() {
                hook();
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nX-Request-Id: role-run-request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    (format!("http://{address}/v1"), handle)
}

#[cfg(unix)]
fn start_hanging_provider() -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<()>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end = loop {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "provider request ended before headers");
            request.extend_from_slice(&chunk[..count]);
            if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "provider request ended before body");
            request.extend_from_slice(&chunk[..count]);
        }
        ready_tx.send(()).unwrap();
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("hanging provider read failed: {error}"),
            }
        }
    });
    (format!("http://{address}/v1"), ready_rx, handle)
}

fn tool_sse(name: &str, id: &str, arguments: serde_json::Value) -> String {
    let delta = serde_json::json!({
        "choices": [{
            "delta": {"tool_calls": [{
                "index": 0,
                "id": id,
                "function": {"name": name, "arguments": arguments.to_string()}
            }]},
            "finish_reason": null
        }]
    });
    let finish = serde_json::json!({
        "choices": [{"delta":{}, "finish_reason":"tool_calls"}]
    });
    format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

fn responses_tool_sse(
    name: &str,
    id: &str,
    arguments: serde_json::Value,
    model: &str,
    service_tier: &str,
) -> String {
    let arguments = arguments.to_string();
    let events = [
        serde_json::json!({
            "type":"response.created",
            "response":{"id":"resp-role-run","model":model,"service_tier":service_tier}
        }),
        serde_json::json!({
            "type":"response.output_item.added",
            "output_index":0,
            "item":{"type":"function_call","call_id":id,"name":name,"arguments":""}
        }),
        serde_json::json!({
            "type":"response.function_call_arguments.done",
            "output_index":0,
            "arguments":arguments
        }),
        serde_json::json!({
            "type":"response.output_item.done",
            "output_index":0,
            "item":{
                "id":"fc-role-run",
                "type":"function_call",
                "call_id":id,
                "name":name,
                "arguments":arguments,
                "status":"completed"
            }
        }),
        serde_json::json!({
            "type":"response.completed",
            "response":{
                "id":"resp-role-run",
                "model":model,
                "service_tier":service_tier,
                "status":"completed",
                "output":[{
                    "id":"fc-role-run",
                    "type":"function_call",
                    "call_id":id,
                    "name":name,
                    "arguments":arguments,
                    "status":"completed"
                }],
                "usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}
            }
        }),
    ];
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect()
}

fn fixture_request(
    workspace: &std::path::Path,
    schema: &std::path::Path,
    tools: serde_json::Value,
    grants: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "schema": "ca.role_run.v1",
        "request_id": "worker-INT-148-001",
        "provider": "fake",
        "model": "fake-model",
        "reasoning_effort": "high",
        "workspace": workspace,
        "prompt": "Return the requested structured result.",
        "output_schema": schema,
        "tools": tools,
        "grants": grants
    })
}

fn install_schema(workspace: &std::path::Path) -> std::path::PathBuf {
    let directory = workspace.join(".orchestrator/schemas");
    std::fs::create_dir_all(&directory).unwrap();
    let schema = directory.join("worker.schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","additionalProperties":false,"required":["answer"],"properties":{"answer":{"type":"integer"}}}"#,
    )
    .unwrap();
    schema
}

fn parse_jsonl(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect()
}

fn assert_successful_role_jsonl(output: &Output, expected: serde_json::Value) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let events = parse_jsonl(output);
    assert!(!events.is_empty());

    let run_id = events[0]["run_id"].as_str().unwrap();
    assert!(!run_id.is_empty());
    uuid::Uuid::parse_str(run_id).unwrap();
    for (sequence, event) in events.iter().enumerate() {
        assert_eq!(event["schema"], "ca.headless.v2");
        assert_eq!(event["sequence"], u64::try_from(sequence).unwrap());
        assert_eq!(event["run_id"], run_id);
    }

    let structured = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["event"]["type"] == "structured_output")
        .collect::<Vec<_>>();
    assert_eq!(structured.len(), 1);
    assert!(structured[0].1["event"]["value"].is_object());
    assert_eq!(structured[0].1["event"]["value"], expected);

    let completion = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event["event"]["type"] == "completion")
        .collect::<Vec<_>>();
    assert_eq!(completion.len(), 1);
    assert_eq!(completion[0].0, events.len() - 1);
    assert!(completion[0].0 > structured[0].0);
    assert!(events.iter().all(|event| !matches!(
        event["event"]["type"].as_str(),
        Some("failure" | "approval_required")
    )));
}

#[test]
fn role_run_preflight_is_offline_redacted_and_state_free() {
    let home = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let profile = install_role_profile(
        home.path(),
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        "",
    );
    let profile_text = std::fs::read_to_string(&profile)
        .unwrap()
        .replace("${ROLE_RUN_TEST_KEY}", "${ROLE_RUN_MISSING_KEY}");
    std::fs::write(&profile, profile_text).unwrap();
    let before = std::fs::read(&profile).unwrap();

    let mut command = ca_command(home.path());
    command.env_remove("ROLE_RUN_MISSING_KEY").args([
        "role-run",
        "--profile",
        "codex-team",
        "--preflight",
    ]);
    let output = command.output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["schema"], "ca.role_run_preflight.v1");
    assert_eq!(receipt["ready"], true);
    assert_eq!(receipt["profile"], "codex-team");
    assert_eq!(receipt["sandbox_required"], true);
    assert_eq!(receipt["routes"][0]["provider"], "fake");
    assert_eq!(
        receipt["routes"][0]["models"],
        serde_json::json!(["fake-model"])
    );
    let output_text = String::from_utf8_lossy(&output.stdout);
    assert!(!output_text.contains("ROLE-RUN-SECRET-CANARY"));
    assert_eq!(std::fs::read(profile).unwrap(), before);
    assert!(!home.path().join(".local/share/ca").exists());
    let connection = listener.accept().unwrap_err();
    assert_eq!(connection.kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn role_run_help_and_reserved_profile_rejection_are_local() {
    let home = tempfile::tempdir().unwrap();
    let root_help = ca_command(home.path()).arg("--help").output().unwrap();
    assert!(root_help.status.success());
    assert!(String::from_utf8(root_help.stdout)
        .unwrap()
        .contains("role-run --profile NAME"));

    let help = ca_command(home.path())
        .args(["role-run", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--profile <NAME>"));
    assert!(help.contains("--preflight"));

    let rejected = ca_command(home.path())
        .args(["role-run", "--profile", "default", "--preflight"])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("nonreserved named profile"));
    assert!(!home.path().join(".config/ca").exists());
    assert!(!home.path().join(".local/share/ca").exists());

    let missing_profile = ca_command(home.path()).arg("role-run").output().unwrap();
    assert_eq!(missing_profile.status.code(), Some(2));
    assert!(missing_profile.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing_profile.stderr).contains("--profile <NAME>"));
    assert!(!home.path().join(".config/ca").exists());
    assert!(!home.path().join(".local/share/ca").exists());
}

#[cfg(unix)]
#[test]
fn role_run_rejects_a_hardlinked_output_schema_before_provider_traffic() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(workspace.join(".orchestrator/schemas")).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let outside = home.path().join("outside-schema.json");
    let schema = workspace.join(".orchestrator/schemas/worker.schema.json");
    let canary = "OUTSIDE-SCHEMA-CANARY";
    std::fs::write(
        &outside,
        format!(r#"{{"type":"object","description":"{canary}"}}"#),
    )
    .unwrap();
    std::fs::hard_link(&outside, &schema).unwrap();
    install_role_profile(home.path(), "http://127.0.0.1:9/v1", "");
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["read_file"]),
        serde_json::json!([{"kind":"tool","tool":"read_file"}]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stdout.contains("role-run output schema must be a contained regular non-symlink schema"),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(!stdout.contains(canary));
    assert!(!stderr.contains(canary));
}

#[test]
fn role_run_preflight_unready_receipts_are_successful_typed_results() {
    let cases = [
        (
            "sandbox_not_required",
            "sandbox_mode = \"required\"",
            "sandbox_mode = \"policy-only\"",
            false,
        ),
        (
            "no_configured_routes",
            "supports_tools = true",
            "supports_tools = false",
            true,
        ),
    ];

    for (issue, before, after, sandbox_required) in cases {
        let home = tempfile::tempdir().unwrap();
        let profile = install_role_profile(home.path(), "http://127.0.0.1:1/v1", "");
        let profile_text = std::fs::read_to_string(&profile)
            .unwrap()
            .replace(before, after)
            .replace("${ROLE_RUN_TEST_KEY}", "${ROLE_RUN_MISSING_KEY}");
        std::fs::write(&profile, profile_text).unwrap();
        let profile_before = std::fs::read(&profile).unwrap();

        let mut command = ca_command(home.path());
        command.env_remove("ROLE_RUN_MISSING_KEY").args([
            "role-run",
            "--profile",
            "codex-team",
            "--preflight",
        ]);
        let output = command.output().unwrap();

        assert_eq!(output.status.code(), Some(0), "case {issue}");
        assert!(output.stderr.is_empty(), "case {issue}");
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["schema"], "ca.role_run_preflight.v1");
        assert_eq!(receipt["ready"], false);
        assert_eq!(receipt["profile"], "codex-team");
        assert_eq!(receipt["sandbox_required"], sandbox_required);
        assert!(receipt["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some(issue)));
        if issue == "no_configured_routes" {
            assert_eq!(receipt["routes"], serde_json::json!([]));
        }
        assert_eq!(std::fs::read(profile).unwrap(), profile_before);
        assert!(!home.path().join(".local/share/ca").exists());
    }
}

#[test]
fn role_run_advertises_only_requested_tools_and_requires_structured_output() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let schema = install_schema(&workspace);
    let (base_url, provider) = start_fake_provider(vec![tool_sse(
        "report_structured_output",
        "structured-1",
        serde_json::json!({"value":{"answer":42}}),
    )]);
    let profile = install_role_profile(home.path(), &base_url, "");
    let before = std::fs::read(&profile).unwrap();
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["read_file"]),
        serde_json::json!([{"kind":"tool", "tool":"read_file"}]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_successful_role_jsonl(&output, serde_json::json!({"answer":42}));
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value =
        serde_json::from_str(requests[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let tool_names = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(tool_names, ["read_file", "report_structured_output"]);
    assert_eq!(body["reasoning_effort"], "high");
    assert_eq!(std::fs::read(profile).unwrap(), before);
    assert!(!home.path().join(".local/share/ca").exists());
}

#[test]
fn role_run_provider_protocol_failure_does_not_echo_a_credential_canary() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let schema = install_schema(&workspace);
    let (base_url, provider) = start_fake_provider(vec!["data: ROLE-RUN-SECRET-CANARY\n\n".into()]);
    install_role_profile(home.path(), &base_url, "");
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["read_file"]),
        serde_json::json!([{"kind":"tool", "tool":"read_file"}]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_eq!(output.status.code(), Some(6));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("provider_protocol"));
    assert!(!stdout.contains("ROLE-RUN-SECRET-CANARY"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("ROLE-RUN-SECRET-CANARY"));
    assert_eq!(provider.join().unwrap().len(), 1);
}

#[test]
fn role_run_success_redacts_the_resolved_credential_from_structured_result_and_metadata() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let schema_dir = workspace.join(".orchestrator/schemas");
    std::fs::create_dir_all(&schema_dir).unwrap();
    let schema = schema_dir.join("secret.schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","additionalProperties":false,"required":["answer"],"properties":{"answer":{"type":"string"}}}"#,
    )
    .unwrap();
    let canary = "ROLE-RUN-SECRET-CANARY";
    let (base_url, provider) = start_fake_provider(vec![responses_tool_sse(
        "report_structured_output",
        &format!("call-{canary}"),
        serde_json::json!({"value":{"answer":canary}}),
        canary,
        canary,
    )]);
    let profile = install_role_profile(home.path(), &base_url, "");
    let profile_text = std::fs::read_to_string(&profile).unwrap().replace(
        "api_key = \"${ROLE_RUN_TEST_KEY}\"\nmodels",
        "api_key = \"${ROLE_RUN_TEST_KEY}\"\nprotocol = \"openai_responses\"\nmodels",
    );
    std::fs::write(&profile, profile_text).unwrap();
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["read_file"]),
        serde_json::json!([{"kind":"tool", "tool":"read_file"}]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(canary));
    assert!(!stderr.contains(canary));
    let events = stdout
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(events.iter().any(|event| {
        event["event"]["type"] == "structured_output"
            && event["event"]["value"]["answer"] == "[REDACTED]"
    }));
    assert!(events.iter().any(|event| {
        event["event"]["type"] == "usage"
            && event["event"]["usage"]["response_metadata"]["model"] == "redacted"
            && event["event"]["usage"]["response_metadata"]["service_tier"] == "redacted"
    }));
    assert_eq!(provider.join().unwrap().len(), 1);
}

#[test]
fn role_run_persistent_allow_cannot_widen_exact_process_grant() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let allowed = workspace.join("allowed");
    std::fs::create_dir(&allowed).unwrap();
    let blocked = workspace.join("blocked.txt");
    let schema = install_schema(&workspace);
    let (base_url, provider) = start_fake_provider(vec![tool_sse(
        "write_file",
        "write-1",
        serde_json::json!({"path":blocked, "content":"must not be written"}),
    )]);
    let policy = format!(
        "permission_rules = [{{ decision = \"allow\", kind = \"path\", tool = \"write_file\", path_pattern = \"{}/**\" }}]",
        workspace.display()
    );
    install_role_profile(home.path(), &base_url, &policy);
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["apply_patch", "write_file"]),
        serde_json::json!([
            {
                "kind":"path",
                "tool":"apply_patch",
                "path_pattern":format!("{}/**", allowed.display())
            },
            {
                "kind":"path",
                "tool":"write_file",
                "path_pattern":format!("{}/**", allowed.display())
            }
        ]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_eq!(output.status.code(), Some(4));
    assert!(!blocked.exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("approval_required"));
    assert!(!stdout.contains("must not be written"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("must not be written"));
    assert_eq!(provider.join().unwrap().len(), 1);
}

#[test]
fn role_run_exact_write_grant_allows_only_the_named_file() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let target = workspace.join("allowed.txt");
    let schema = install_schema(&workspace);
    let (base_url, provider) = start_fake_provider(vec![
        tool_sse(
            "write_file",
            "write-allowed",
            serde_json::json!({"path":target, "content":"bounded write"}),
        ),
        tool_sse(
            "report_structured_output",
            "structured-allowed",
            serde_json::json!({"value":{"answer":7}}),
        ),
    ]);
    install_role_profile(home.path(), &base_url, "");
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["apply_patch", "write_file"]),
        serde_json::json!([
            {"kind":"path", "tool":"apply_patch", "path_pattern":target},
            {"kind":"path", "tool":"write_file", "path_pattern":target}
        ]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_successful_role_jsonl(&output, serde_json::json!({"answer":7}));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "bounded write");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("bounded write"));
    assert!(!stdout.contains("encoded_bytes"));
    assert!(!stdout.contains("role-run tool output redacted;"));
    assert!(stdout.contains("\"call_id\":\"role-call-1-0\""));
    assert_eq!(provider.join().unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn role_run_write_tools_reject_hardlinked_targets_without_changing_the_outside_inode() {
    for tool in ["write_file", "apply_patch"] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let outside = home.path().join(format!("outside-{tool}.txt"));
        let target = workspace.join(format!("linked-{tool}.txt"));
        std::fs::write(&outside, "outside remains unchanged").unwrap();
        std::fs::hard_link(&outside, &target).unwrap();
        let schema = install_schema(&workspace);
        let arguments = match tool {
            "write_file" => {
                serde_json::json!({"path":target,"content":"blocked hardlink write"})
            }
            "apply_patch" => serde_json::json!({
                "path":target,
                "patch":"@@ -1 +1 @@\n-outside remains unchanged\n+blocked hardlink patch"
            }),
            _ => unreachable!(),
        };
        let (base_url, provider) = start_fake_provider(vec![
            tool_sse(tool, "hardlink-write", arguments),
            tool_sse(
                "report_structured_output",
                "hardlink-result",
                serde_json::json!({"value":{"answer":11}}),
            ),
        ]);
        install_role_profile(home.path(), &base_url, "");
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!(["apply_patch", "write_file"]),
            serde_json::json!([
                {"kind":"path", "tool":"apply_patch", "path_pattern":target},
                {"kind":"path", "tool":"write_file", "path_pattern":target}
            ]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_successful_role_jsonl(&output, serde_json::json!({"answer":11}));
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "outside remains unchanged"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "outside remains unchanged"
        );
        assert!(parse_jsonl(&output).iter().any(|event| {
            event["event"]["type"] == "tool_finish" && event["event"]["status"] == "failure"
        }));
        assert_eq!(provider.join().unwrap().len(), 2);
    }
}

#[cfg(unix)]
#[test]
fn role_run_write_tools_reject_a_target_swapped_to_a_symlink() {
    use std::os::unix::fs::symlink;

    for tool in ["write_file", "apply_patch"] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let outside = home.path().join(format!("outside-symlink-{tool}.txt"));
        let target = workspace.join(format!("swapped-{tool}.txt"));
        std::fs::write(&outside, "outside remains unchanged").unwrap();
        std::fs::write(&target, "initially valid").unwrap();
        let schema = install_schema(&workspace);
        let arguments = match tool {
            "write_file" => serde_json::json!({"path":target,"content":"blocked symlink write"}),
            "apply_patch" => serde_json::json!({
                "path":target,
                "patch":"@@ -1 +1 @@\n-outside remains unchanged\n+blocked symlink patch"
            }),
            _ => unreachable!(),
        };
        let swapped_target = target.clone();
        let outside_target = outside.clone();
        let (base_url, provider) = start_fake_provider_with_first_request_hook(
            vec![tool_sse(tool, "symlink-write", arguments)],
            Some(Box::new(move || {
                std::fs::remove_file(&swapped_target).unwrap();
                symlink(&outside_target, &swapped_target).unwrap();
            })),
        );
        install_role_profile(home.path(), &base_url, "");
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!(["apply_patch", "write_file"]),
            serde_json::json!([
                {"kind":"path", "tool":"apply_patch", "path_pattern":target},
                {"kind":"path", "tool":"write_file", "path_pattern":target}
            ]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_eq!(output.status.code(), Some(6));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("role-run tool request was invalid"));
        assert!(!stdout.contains("blocked symlink"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("blocked symlink"));
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "outside remains unchanged"
        );
        assert!(std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(provider.join().unwrap().len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn role_run_read_tools_reject_hardlinks_without_disclosing_outside_bytes() {
    for tool in [
        "read_file",
        "list_dir",
        "search_files",
        "glob",
        "read_todos",
    ] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        let scope = workspace.join("scope");
        std::fs::create_dir_all(&scope).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let scope = workspace.join("scope");
        let outside = home.path().join(format!("outside-read-{tool}.txt"));
        let linked = scope.join("private.txt");
        let canary = format!("OUTSIDE-READ-CANARY-{tool} TODO");
        std::fs::write(&outside, &canary).unwrap();
        std::fs::hard_link(&outside, &linked).unwrap();
        let schema = install_schema(&workspace);
        let arguments = match tool {
            "read_file" => serde_json::json!({"path":linked}),
            "list_dir" => serde_json::json!({"path":scope}),
            "search_files" => serde_json::json!({"path":scope,"pattern":"CANARY"}),
            "glob" => serde_json::json!({"path":scope,"pattern":"**/*"}),
            "read_todos" => serde_json::json!({"path":scope}),
            _ => unreachable!(),
        };
        let (base_url, provider) = start_fake_provider(vec![
            tool_sse(tool, "hardlink-read", arguments),
            tool_sse(
                "report_structured_output",
                "hardlink-read-result",
                serde_json::json!({"value":{"answer":17}}),
            ),
        ]);
        install_role_profile(home.path(), &base_url, "");
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!([tool]),
            serde_json::json!([{"kind":"tool","tool":tool}]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_successful_role_jsonl(&output, serde_json::json!({"answer":17}));
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stdout.contains("\"status\":\"failure\""));
        assert!(!stdout.contains(&canary), "tool {tool}");
        assert!(!stderr.contains(&canary), "tool {tool}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), canary);
        assert_eq!(provider.join().unwrap().len(), 2);
    }
}

#[cfg(unix)]
#[test]
fn role_run_read_tools_reject_targets_swapped_to_outside_symlinks() {
    use std::os::unix::fs::symlink;

    for tool in [
        "read_file",
        "list_dir",
        "search_files",
        "glob",
        "read_todos",
    ] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let outside_file = home.path().join(format!("outside-swap-{tool}.txt"));
        let outside_directory = home.path().join(format!("outside-swap-{tool}"));
        let canary = format!("OUTSIDE-SWAP-CANARY-{tool} TODO");
        std::fs::write(&outside_file, &canary).unwrap();
        std::fs::create_dir(&outside_directory).unwrap();
        std::fs::write(outside_directory.join("private.txt"), &canary).unwrap();
        let target = workspace.join(format!("swapped-{tool}"));
        let arguments = if tool == "read_file" {
            std::fs::write(&target, "initially safe").unwrap();
            serde_json::json!({"path":target})
        } else {
            std::fs::create_dir(&target).unwrap();
            std::fs::write(target.join("safe.txt"), "initially safe").unwrap();
            match tool {
                "list_dir" | "read_todos" => serde_json::json!({"path":target}),
                "search_files" => serde_json::json!({"path":target,"pattern":"CANARY"}),
                "glob" => serde_json::json!({"path":target,"pattern":"**/*"}),
                _ => unreachable!(),
            }
        };
        let swapped_target = target.clone();
        let outside_target = if tool == "read_file" {
            outside_file.clone()
        } else {
            outside_directory.clone()
        };
        let (base_url, provider) = start_fake_provider_with_first_request_hook(
            vec![tool_sse(tool, "symlink-read", arguments)],
            Some(Box::new(move || {
                if swapped_target.is_dir() {
                    std::fs::remove_dir_all(&swapped_target).unwrap();
                } else {
                    std::fs::remove_file(&swapped_target).unwrap();
                }
                symlink(&outside_target, &swapped_target).unwrap();
            })),
        );
        install_role_profile(home.path(), &base_url, "");
        let schema = install_schema(&workspace);
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!([tool]),
            serde_json::json!([{"kind":"tool","tool":tool}]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_eq!(
            output.status.code(),
            Some(6),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stdout.contains("role-run tool request was invalid"));
        assert!(!stdout.contains(&canary), "tool {tool}");
        assert!(!stderr.contains(&canary), "tool {tool}");
        assert!(std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(provider.join().unwrap().len(), 1);
    }
}

#[test]
fn role_run_rejects_forged_write_routing_arguments_before_mutation() {
    for (tool, forged_key) in [("write_file", "cwd"), ("apply_patch", "dest")] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let target = workspace.join("exact-grant.txt");
        let target_canary = format!("BLOCKED-TARGET-CANARY-{tool}");
        std::fs::write(&target, &target_canary).unwrap();
        let outside = home.path().join("outside-canary.txt");
        let outside_canary = format!("OUTSIDE-PATH-CANARY-{tool}");
        std::fs::write(&outside, &outside_canary).unwrap();
        let schema = install_schema(&workspace);
        let forged_content = format!("FORGED-WRITE-CANARY-{tool}");
        let mut arguments = match tool {
            "write_file" => serde_json::json!({
                "path": target,
                "content": forged_content,
            }),
            "apply_patch" => serde_json::json!({
                "path": target,
                "patch": format!("@@ -1 +1 @@\n-{target_canary}\n+{forged_content}"),
            }),
            _ => unreachable!(),
        };
        arguments
            .as_object_mut()
            .unwrap()
            .insert(forged_key.into(), serde_json::json!(outside));
        let (base_url, provider) =
            start_fake_provider(vec![tool_sse(tool, "forged-routing", arguments)]);
        install_role_profile(home.path(), &base_url, "");
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!(["apply_patch", "write_file"]),
            serde_json::json!([
                {"kind":"path", "tool":"apply_patch", "path_pattern":target},
                {"kind":"path", "tool":"write_file", "path_pattern":target}
            ]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_eq!(output.status.code(), Some(6), "tool {tool}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), target_canary);
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), outside_canary);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stdout.contains("provider_protocol"), "tool {tool}");
        for canary in [&target_canary, &outside_canary, &forged_content] {
            assert!(!stdout.contains(canary), "tool {tool}");
            assert!(!stderr.contains(canary), "tool {tool}");
        }
        assert_eq!(provider.join().unwrap().len(), 1);
    }
}

#[test]
fn role_run_rejects_selected_credential_echoes_in_write_tool_arguments() {
    let canary = "ROLE-RUN-SECRET-CANARY";
    for tool in ["write_file", "apply_patch"] {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let workspace = workspace.canonicalize().unwrap();
        let target = workspace.join(format!("credential-echo-{tool}.txt"));
        std::fs::write(&target, "stable").unwrap();
        let schema = install_schema(&workspace);
        let arguments = match tool {
            "write_file" => serde_json::json!({"path":target,"content":canary}),
            "apply_patch" => serde_json::json!({
                "path":target,
                "patch":format!("@@ -1 +1 @@\n-stable\n+{canary}")
            }),
            _ => unreachable!(),
        };
        let (base_url, provider) =
            start_fake_provider(vec![tool_sse(tool, "credential-echo", arguments)]);
        install_role_profile(home.path(), &base_url, "");
        let request = fixture_request(
            &workspace,
            &schema,
            serde_json::json!(["apply_patch", "write_file"]),
            serde_json::json!([
                {"kind":"path", "tool":"apply_patch", "path_pattern":target},
                {"kind":"path", "tool":"write_file", "path_pattern":target}
            ]),
        );

        let output = run_role(home.path(), &workspace, &request);

        assert_eq!(output.status.code(), Some(6));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "stable");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stdout.contains("role-run tool request was invalid"));
        assert!(!stdout.contains(canary));
        assert!(!stderr.contains(canary));
        assert_eq!(provider.join().unwrap().len(), 1);
    }
}

#[test]
fn role_run_persistent_deny_remains_a_ceiling_over_an_exact_grant() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let target = workspace.join("denied.txt");
    let schema = install_schema(&workspace);
    let (base_url, provider) = start_fake_provider(vec![
        tool_sse(
            "write_file",
            "write-denied",
            serde_json::json!({"path":target, "content":"denied secret canary"}),
        ),
        tool_sse(
            "report_structured_output",
            "structured-denied",
            serde_json::json!({"value":{"answer":9}}),
        ),
    ]);
    let policy = format!(
        "permission_rules = [{{ decision = \"deny\", kind = \"path\", tool = \"write_file\", path_pattern = \"{}\" }}]",
        target.display()
    );
    install_role_profile(home.path(), &base_url, &policy);
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["apply_patch", "write_file"]),
        serde_json::json!([
            {"kind":"path", "tool":"apply_patch", "path_pattern":target},
            {"kind":"path", "tool":"write_file", "path_pattern":target}
        ]),
    );

    let output = run_role(home.path(), &workspace, &request);

    assert_successful_role_jsonl(&output, serde_json::json!({"answer":9}));
    assert!(!target.exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"status\":\"denied\""));
    assert!(!stdout.contains("denied secret canary"));
    assert_eq!(provider.join().unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn role_run_sigterm_returns_typed_interruption_without_runtime_state() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let schema = install_schema(&workspace);
    let (base_url, provider_ready, provider) = start_hanging_provider();
    install_role_profile(home.path(), &base_url, "");
    let request = fixture_request(
        &workspace,
        &schema,
        serde_json::json!(["read_file"]),
        serde_json::json!([{"kind":"tool", "tool":"read_file"}]),
    );
    let mut child = ca_command(home.path())
        .current_dir(&workspace)
        .args(["role-run", "--profile", "codex-team"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.as_mut().unwrap(), &request).unwrap();
    drop(child.stdin.take());
    provider_ready
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap()),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("role-run did not stop after SIGTERM");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();

    assert_eq!(status.code(), Some(130));
    assert!(stderr.is_empty());
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(stdout.contains("\"kind\":\"interrupted\""));
    assert!(stdout.contains("\"exit_code\":130"));
    assert!(!home.path().join(".local/share/ca").exists());
    provider.join().unwrap();
}
