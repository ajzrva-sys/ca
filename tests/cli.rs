use std::process::Command;

fn install_fake_config(home: &std::path::Path, base_url: &str, max_turns: usize) {
    let config_dir = home.join(".config/ca");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"approval_rules = []

[defaults]
provider = "fake"
model = "fake-model"

[agent]
max_turns = {max_turns}
approval_mode = "allowlist"
command_timeout_secs = 2

[providers.fake]
base_url = "{base_url}"
api_key = "test-only"
models = ["fake-model"]
"#
        ),
    )
    .unwrap();
}

fn start_fake_provider(responses: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
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
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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

fn text_sse(text: &str) -> String {
    let delta = serde_json::json!({
        "choices": [{"delta":{"content":text}, "finish_reason":null}]
    });
    let finish = serde_json::json!({
        "choices": [{"delta":{}, "finish_reason":"stop"}],
        "usage": {"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}
    });
    format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
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

fn ca_command(home: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ca"));
    command.env("HOME", home);
    command
}

#[test]
fn archive_learning_help_is_local_and_does_not_create_profile_state() {
    let home = tempfile::tempdir().unwrap();
    let help = ca_command(home.path())
        .args(["learn", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["source", "sync", "status", "review", "proposal", "service"] {
        assert!(help.contains(command), "learn help omitted {command}");
    }
    assert!(!home.path().join(".local/share/ca").exists());
}

#[test]
fn runtime_help_and_empty_list_are_local_and_do_not_create_state() {
    let home = tempfile::tempdir().unwrap();
    let help = ca_command(home.path())
        .args(["runtime", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in [
        "trust", "untrust", "sync", "list", "preview", "install", "update", "rollback", "path",
        "remove",
    ] {
        assert!(help.contains(command), "runtime help omitted {command}");
    }

    let list = ca_command(home.path())
        .args(["runtime", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    assert_eq!(
        String::from_utf8(list.stdout).unwrap().trim(),
        "No runtime catalogs or managed runtimes configured"
    );
    assert!(
        !home.path().join(".local/share/ca").exists(),
        "read-only list created runtime state"
    );
}

#[test]
fn local_model_help_empty_list_and_import_lifecycle_are_explicit_and_profile_scoped() {
    let home = tempfile::tempdir().unwrap();
    let help = ca_command(home.path())
        .args(["local", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["list", "inspect", "import", "download", "remove", "serve"] {
        assert!(help.contains(command), "local help omitted {command}");
    }

    let list = ca_command(home.path())
        .args(["local", "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    assert_eq!(
        String::from_utf8(list.stdout).unwrap().trim(),
        "No managed local GGUF models"
    );
    assert!(!home.path().join(".local/share/ca").exists());

    let source = home.path().join("source.gguf");
    let mut gguf = b"GGUF".to_vec();
    gguf.extend_from_slice(&3_u32.to_le_bytes());
    gguf.extend_from_slice(&2_u64.to_le_bytes());
    gguf.extend_from_slice(&4_u64.to_le_bytes());
    gguf.extend_from_slice(b"test-only-payload");
    std::fs::write(&source, &gguf).unwrap();
    let imported = ca_command(home.path())
        .args([
            "local",
            "--profile",
            "work",
            "import",
            source.to_str().unwrap(),
            "--id",
            "demo",
            "--confirm",
            "demo",
        ])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(String::from_utf8_lossy(&imported.stdout).contains("nothing was started"));
    assert_eq!(std::fs::read(&source).unwrap(), gguf);
    assert!(!home.path().join(".config/ca").exists());
    assert!(!home.path().join(".local/share/ca/local-models").exists());

    let inspected = ca_command(home.path())
        .args(["local", "--profile", "work", "inspect", "demo"])
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let inspected = String::from_utf8(inspected.stdout).unwrap();
    assert!(inspected.contains("GGUF v3"));
    assert!(inspected.contains("excludes KV cache and GPU/runtime overhead"));

    let removed = ca_command(home.path())
        .args([
            "local",
            "--profile",
            "work",
            "remove",
            "demo",
            "--confirm",
            "demo",
        ])
        .output()
        .unwrap();
    assert!(removed.status.success());
    assert!(source.exists());
    let list = ca_command(home.path())
        .args(["local", "--profile", "work", "list"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(list.stdout).unwrap().trim(),
        "No managed local GGUF models"
    );
}

#[test]
fn update_help_and_status_are_local_and_do_not_create_state() {
    let home = tempfile::tempdir().unwrap();
    let help = ca_command(home.path())
        .args(["update", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in [
        "trust", "status", "check", "channel", "pin", "download", "install", "rollback",
    ] {
        assert!(help.contains(command), "update help omitted {command}");
    }
    let status = ca_command(home.path())
        .args(["update", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("authority: none"));
    assert!(!home.path().join(".local/share/ca").exists());
    assert!(!home.path().join(".config/ca").exists());
}

fn enable_policy_only_sandbox(home: &std::path::Path) {
    let path = home.join(".config/ca/config.toml");
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        raw.replace(
            "command_timeout_secs = 2",
            "command_timeout_secs = 2\nsandbox_mode = \"policy-only\"",
        ),
    )
    .unwrap();
}

fn install_fake_mcp(home: &std::path::Path, script: &std::path::Path, marker: &std::path::Path) {
    use std::io::Write;
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(home.join(".config/ca/config.toml"))
        .unwrap();
    write!(
        config,
        r#"
[mcp.servers.fake]
command = "{}"
args = ["{}"]
env = {{ EXIT_MARKER = "{}", TEST_MCP_SECRET = "resolved-secret" }}
timeout_secs = 2
network = false
"#,
        python3_executable().display(),
        script.display(),
        marker.display()
    )
    .unwrap();
}

fn python3_executable() -> std::path::PathBuf {
    ["/usr/bin/python3", "/usr/local/bin/python3"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|path| path.is_file())
        .expect("Python 3 CLI fixture executable")
}

fn write_fake_mcp_server(workspace: &std::path::Path) -> std::path::PathBuf {
    let script = workspace.join("fake_mcp.py");
    std::fs::write(
        &script,
        r#"import json, os, sys
def send(value):
    sys.stdout.write(json.dumps(value, separators=(",", ":")) + "\n")
    sys.stdout.flush()
try:
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        request_id = request.get("id")
        if method == "initialize":
            send({"jsonrpc":"2.0","id":request_id,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"fake","version":"1"}}})
        elif method == "tools/list":
            send({"jsonrpc":"2.0","id":request_id,"result":{"tools":[{"name":"echo","description":"Echo","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]}})
        elif method == "resources/list":
            send({"jsonrpc":"2.0","id":request_id,"result":{"resources":[]}})
        elif method == "tools/call":
            send({"jsonrpc":"2.0","id":request_id,"result":{"content":[{"type":"text","text":request["params"]["arguments"]["text"]}],"isError":False}})
finally:
    with open(os.environ["EXIT_MARKER"], "w") as handle:
        handle.write("closed")
"#,
    )
    .unwrap();
    script
}

#[test]
fn help_is_local_and_names_script_file_contract() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .arg("--help")
        .output()
        .expect("run ca --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--test <SCRIPT_FILE>"));
    assert!(stdout.contains("--skill <ID>"));
    assert!(stdout.contains("--doctor"));
    assert!(stdout.contains("--connect"));
    assert!(stdout.contains("--profile <NAME>"));
    assert!(stdout.contains("--tools <TOOL[,TOOL]>"));
    assert!(stdout.contains("--include-tools <TOOL[,TOOL]>"));
    assert!(stdout.contains("--exclude-tools <TOOL[,TOOL]>"));
    assert!(stdout.contains("--check"));
    assert!(stdout.contains("--check-repair"));
    assert!(stdout.contains("--output-schema <PATH>"));
    assert!(stdout.contains("serve [--stdio | --listen IP:PORT]"));
    assert!(stdout.contains("attach ws://IP:PORT"));
    assert!(stdout.contains("schedule <COMMAND>"));
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn scheduled_prompt_is_disabled_by_default_then_runs_in_an_isolated_worker() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (base_url, provider) = start_fake_provider(vec![text_sse("scheduled-ok")]);
    install_fake_config(home.path(), &base_url, 2);
    let definition = workspace.join("nightly.toml");
    std::fs::write(
        &definition,
        format!(
            r#"version = 1
id = "nightly"
provider = "fake"
model = "fake-model"
workspace = "{}"
runtime_secs = 10
grace_secs = 60
tools = []
grants = []
allow_network = false

[trigger]
kind = "once"
at = "2099-01-01T00:00:00Z"

[target]
kind = "prompt"
prompt = "scheduled semantic input"

[delivery]
kind = "local_file"
path = "results/nightly.json"
"#,
            workspace.canonicalize().unwrap().display()
        ),
    )
    .unwrap();

    let help = ca_command(home.path())
        .args(["schedule", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Manage explicit"));

    let added = ca_command(home.path())
        .args(["schedule", "add", definition.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(String::from_utf8_lossy(&added.stdout).contains("disabled"));
    assert!(!workspace.join("results/nightly.json").exists());

    let listed = ca_command(home.path())
        .args(["schedule", "list"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listed.stdout);
    assert!(listing.contains("nightly\tdisabled\tfake/fake-model"));
    assert!(!listing.contains("scheduled semantic input"));

    let run = ca_command(home.path())
        .args(["schedule", "run", "nightly"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let result_path = workspace.join("results/nightly.json");
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&result_path).unwrap()).unwrap();
    assert_eq!(result["schema"], "ca.schedule.result.v1");
    assert_eq!(result["schedule_id"], "nightly");
    assert_eq!(result["provider"], "fake");
    assert_eq!(result["model"], "fake-model");
    assert_eq!(result["content"], "scheduled-ok");
    assert!(!home.path().join(".local/share/ca/sessions").exists());

    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("scheduled semantic input"));
    assert!(!requests[0].contains("\"tools\""));
}

#[test]
fn local_plugin_is_inert_confirmed_and_composed_without_native_code() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    let bundle = home.path().join("bundle");
    let skill = bundle.join("skills/plugin-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: plugin-skill\ndescription: Plugin test skill\n---\nPLUGIN-BODY-SENTINEL",
    )
    .unwrap();
    std::fs::write(
        bundle.join("workflow.toml"),
        r#"version = 1
id = "plugin-flow"
title = "Plugin flow"

[[steps]]
id = "answer"
prompt = "answer safely"
tools = []
"#,
    )
    .unwrap();
    std::fs::write(
        bundle.join("plugin.toml"),
        r#"version = 1
id = "daily-tools"
name = "Daily tools"
description = "Local test bundle"
skills = ["skills"]

[workflows]
answer = "workflow.toml"

[themes]
midnight = "ocean"
"#,
    )
    .unwrap();
    let (base_url, provider) = start_fake_provider(vec![text_sse("plugin-ok")]);
    install_fake_config(home.path(), &base_url, 2);

    let help = ca_command(home.path())
        .args(["plugin", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let added = ca_command(home.path())
        .args([
            "plugin",
            "add",
            bundle.join("plugin.toml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let list = ca_command(home.path())
        .args(["plugin", "list"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&list.stdout).contains("daily-tools\tdisabled"));
    assert!(!String::from_utf8_lossy(&list.stdout).contains("PLUGIN-BODY-SENTINEL"));

    let refused = ca_command(home.path())
        .args(["plugin", "enable", "daily-tools", "--confirm", "wrong"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    let enabled = ca_command(home.path())
        .args([
            "plugin",
            "enable",
            "daily-tools",
            "--confirm",
            "daily-tools",
        ])
        .output()
        .unwrap();
    assert!(
        enabled.status.success(),
        "{}",
        String::from_utf8_lossy(&enabled.stderr)
    );

    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: plugin-skill\ndescription: Plugin test skill\n---\nCHANGED-SENTINEL",
    )
    .unwrap();
    let drifted = ca_command(home.path())
        .current_dir(&workspace)
        .args(["--skill", "plugin-skill", "-p", "must fail before traffic"])
        .output()
        .unwrap();
    assert_eq!(drifted.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&drifted.stderr).contains("plugin changed after it was added"));
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: plugin-skill\ndescription: Plugin test skill\n---\nPLUGIN-BODY-SENTINEL",
    )
    .unwrap();

    let workflow = ca_command(home.path())
        .args(["plugin", "workflow", "daily-tools/answer"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&workflow.stdout).trim(),
        bundle
            .join("workflow.toml")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    );
    let theme = ca_command(home.path())
        .args(["plugin", "theme", "daily-tools/midnight"])
        .output()
        .unwrap();
    assert!(theme.status.success());
    assert!(
        std::fs::read_to_string(home.path().join(".config/ca/config.toml"))
            .unwrap()
            .contains("theme = \"ocean\"")
    );

    let run = ca_command(home.path())
        .current_dir(&workspace)
        .args(["--skill", "plugin-skill", "-p", "use the plugin"])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("PLUGIN-BODY-SENTINEL"));
    assert!(!home.path().join(".local/share/ca/sessions").exists());

    let disabled = ca_command(home.path())
        .args(["plugin", "disable", "daily-tools"])
        .output()
        .unwrap();
    assert!(disabled.status.success());
    let removed = ca_command(home.path())
        .args([
            "plugin",
            "remove",
            "daily-tools",
            "--confirm",
            "daily-tools",
        ])
        .output()
        .unwrap();
    assert!(removed.status.success());
    assert!(bundle.join("plugin.toml").exists());
}

#[test]
fn signed_catalog_stages_updates_and_rolls_back_without_auto_activation() {
    use base64::Engine as _;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use sha2::{Digest, Sha256};

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn package(source: &std::path::Path, version: &str, body: &str) -> (String, String) {
        let manifest = "version = 1\nid = \"daily\"\nname = \"Daily\"\nskills = [\"skills\"]\n";
        let skill =
            format!("---\nname: daily-skill\ndescription: Signed catalog skill\n---\n{body}");
        let files = [
            ("plugin.toml", manifest.as_bytes()),
            ("skills/daily-skill/SKILL.md", skill.as_bytes()),
        ]
        .into_iter()
        .map(|(path, bytes)| {
            serde_json::json!({
                "path": path,
                "sha256": digest(bytes),
                "content": base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        })
        .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema": "ca.plugin.package.v1",
            "plugin_id": "daily",
            "version": version,
            "files": files,
        }))
        .unwrap();
        let name = format!("daily-{version}.json");
        std::fs::write(source.join(&name), &bytes).unwrap();
        (name, digest(&bytes))
    }

    fn index(
        source: &std::path::Path,
        pair: &Ed25519KeyPair,
        revision: u64,
        version: &str,
        package: &str,
        sha256: &str,
    ) -> std::path::PathBuf {
        let payload = serde_json::to_vec(&serde_json::json!({
            "catalog_id": "official",
            "revision": revision,
            "entries": [{
                "plugin_id": "daily",
                "version": version,
                "package": package,
                "sha256": sha256,
            }],
        }))
        .unwrap();
        let envelope = serde_json::json!({
            "schema": "ca.plugin.catalog-envelope.v1",
            "payload": base64::engine::general_purpose::STANDARD.encode(&payload),
            "signature": base64::engine::general_purpose::STANDARD.encode(pair.sign(&payload).as_ref()),
        });
        let path = source.join(format!("index-{revision}.json"));
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        path
    }

    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    let source = home.path().join("catalog-source");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&source).unwrap();
    let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let pair = Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap();
    let public_key = source.join("catalog.pub");
    std::fs::write(
        &public_key,
        base64::engine::general_purpose::STANDARD.encode(pair.public_key().as_ref()),
    )
    .unwrap();
    let (v1_package, v1_hash) = package(&source, "1.0.0", "SIGNED-CATALOG-V1");
    let v1_index = index(&source, &pair, 1, "1.0.0", &v1_package, &v1_hash);
    let (base_url, provider) = start_fake_provider(vec![text_sse("catalog-ok")]);
    install_fake_config(home.path(), &base_url, 2);

    let trust = ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "trust",
            "official",
            public_key.to_str().unwrap(),
            "--confirm",
            "official",
        ])
        .output()
        .unwrap();
    assert!(
        trust.status.success(),
        "{}",
        String::from_utf8_lossy(&trust.stderr)
    );
    let sync = ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "sync",
            "official",
            v1_index.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let install = ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "install",
            "official/daily",
            "--confirm",
            "daily",
        ])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(String::from_utf8_lossy(&install.stdout).contains("disabled"));

    let inert = ca_command(home.path())
        .current_dir(&workspace)
        .args([
            "--no-session",
            "--skill",
            "daily-skill",
            "-p",
            "must be inert",
        ])
        .output()
        .unwrap();
    assert_eq!(inert.status.code(), Some(2));

    let (v2_package, v2_hash) = package(&source, "2.0.0", "SIGNED-CATALOG-V2");
    let v2_index = index(&source, &pair, 2, "2.0.0", &v2_package, &v2_hash);
    assert!(ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "sync",
            "official",
            v2_index.to_str().unwrap(),
        ])
        .status()
        .unwrap()
        .success());
    assert!(ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "update",
            "official/daily",
            "--confirm",
            "daily",
        ])
        .status()
        .unwrap()
        .success());
    assert!(ca_command(home.path())
        .args([
            "plugin",
            "catalog",
            "rollback",
            "daily",
            "--to",
            "1.0.0",
            "--confirm",
            "daily",
        ])
        .status()
        .unwrap()
        .success());
    assert!(ca_command(home.path())
        .args(["plugin", "enable", "daily", "--confirm", "daily"])
        .status()
        .unwrap()
        .success());
    let run = ca_command(home.path())
        .current_dir(&workspace)
        .args([
            "--no-session",
            "--skill",
            "daily-skill",
            "-p",
            "use the rolled-back catalog skill",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("SIGNED-CATALOG-V1"));
    assert!(!requests[0].contains("SIGNED-CATALOG-V2"));
    assert!(!home.path().join(".local/share/ca/sessions").exists());
}

#[test]
fn serve_help_is_local_and_released_protocol_is_long_lived_jsonl() {
    use std::io::{BufRead, Write};
    use std::process::Stdio;

    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (base_url, provider) = start_fake_provider(vec![text_sse("service-ok")]);
    install_fake_config(home.path(), &base_url, 2);

    let help = ca_command(home.path())
        .args(["serve", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--stdio"));

    let mut child = ca_command(home.path())
        .args(["serve", "--stdio"])
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(ready["schema"], "ca.serve.v1");
    assert_eq!(ready["event"]["type"], "ready");

    writeln!(
        input,
        "{}",
        serde_json::json!({"schema":"ca.serve.v1","id":"network","op":{"type":"start_session","allow_network":true}})
    )
    .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let refused: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(refused["event"]["type"], "error");
    assert_eq!(refused["event"]["kind"], "setup");

    for operation in [
        serde_json::json!({"schema":"ca.serve.v1","id":"start","op":{"type":"start_session"}}),
        serde_json::json!({"schema":"ca.serve.v1","id":"input","op":{"type":"user_input","text":"hello"}}),
    ] {
        writeln!(input, "{}", operation).unwrap();
        input.flush().unwrap();
    }
    let mut sequence = 2_u64;
    let mut saw_delta = false;
    loop {
        line.clear();
        output.read_line(&mut line).unwrap();
        let event: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(event["sequence"], sequence);
        sequence += 1;
        if event["event"]["type"] == "runner"
            && event["event"]["event"]["type"] == "assistant_output"
        {
            assert_eq!(event["event"]["event"]["delta"], "service-ok");
            saw_delta = true;
        }
        if event["event"]["type"] == "turn_completed" {
            break;
        }
    }
    assert!(saw_delta);
    writeln!(
        input,
        "{}",
        serde_json::json!({"schema":"ca.serve.v1","id":"shutdown","op":{"type":"shutdown"}})
    )
    .unwrap();
    input.flush().unwrap();
    loop {
        line.clear();
        output.read_line(&mut line).unwrap();
        let event: serde_json::Value = serde_json::from_str(&line).unwrap();
        if event["event"]["type"] == "goodbye" {
            break;
        }
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    assert!(!home.path().join(".local/share/ca/sessions").exists());
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("hello"));
}

#[cfg(unix)]
#[test]
fn loopback_websocket_and_attach_are_authenticated_bounded_and_process_only() {
    use std::io::{BufRead, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let (base_url, provider) = start_fake_provider(vec![
        tool_sse(
            "write_file",
            "call-ws-write",
            serde_json::json!({"path":"websocket-approved.txt","content":"approved\n"}),
        ),
        text_sse("websocket-ok"),
    ]);
    install_fake_config(home.path(), &base_url, 2);
    let token_text = "right-token-0123456789-abcdefghijk";
    let wrong_text = "wrong-token-0123456789-abcdefghijk";
    let token = home.path().join("attach.token");
    let wrong = home.path().join("wrong.token");
    std::fs::write(&token, token_text).unwrap();
    std::fs::write(&wrong, wrong_text).unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(&wrong, std::fs::Permissions::from_mode(0o600)).unwrap();

    let attach_help = ca_command(home.path())
        .args(["attach", "--help"])
        .output()
        .unwrap();
    assert!(attach_help.status.success());
    assert!(String::from_utf8_lossy(&attach_help.stdout).contains("--auth-file"));

    let mut server = ca_command(home.path())
        .args([
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--auth-file",
            token.to_str().unwrap(),
        ])
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut server_stderr = std::io::BufReader::new(server.stderr.take().unwrap());
    let mut listening = String::new();
    server_stderr.read_line(&mut listening).unwrap();
    let endpoint = listening
        .trim()
        .strip_prefix("ca serve listening on ")
        .expect("safe listener announcement")
        .to_string();
    assert!(endpoint.starts_with("ws://127.0.0.1:"));
    assert!(!listening.contains(token_text));

    let bad = ca_command(home.path())
        .args(["attach", &endpoint, "--auth-file", wrong.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(4));
    let bad_stderr = String::from_utf8_lossy(&bad.stderr);
    assert!(bad_stderr.contains("authentication or loopback connection failed"));
    assert!(!bad_stderr.contains(token_text));
    assert!(!bad_stderr.contains(wrong_text));

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut origin_request = endpoint.as_str().into_client_request().unwrap();
    origin_request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token_text}").parse().unwrap(),
    );
    origin_request
        .headers_mut()
        .insert("Origin", "https://example.invalid".parse().unwrap());
    let origin = runtime.block_on(tokio_tungstenite::connect_async(origin_request));
    assert!(matches!(
        origin,
        Err(tokio_tungstenite::tungstenite::Error::Http(response))
            if response.status() == http::StatusCode::FORBIDDEN
    ));

    let mut attached = ca_command(home.path())
        .args(["attach", &endpoint, "--auth-file", token.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut attach_input = attached.stdin.take().unwrap();
    let attach_output = std::io::BufReader::new(attached.stdout.take().unwrap());
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in attach_output.lines() {
            if event_tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut recent_events = Vec::new();
    let mut next_event = |server: &mut std::process::Child,
                          attached: &mut std::process::Child|
     -> serde_json::Value {
        // Match the fake provider's existing five-second read bound. A
        // service failure must expose its diagnostic instead of waiting
        // forever for an approval or completion that cannot arrive.
        let result = event_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| format!("websocket event reader stopped: {error}"))
            .and_then(|line| line.map_err(|error| error.to_string()))
            .and_then(|line| {
                recent_events.push(line.clone());
                if recent_events.len() > 64 {
                    recent_events.remove(0);
                }
                serde_json::from_str::<serde_json::Value>(&line)
                    .map_err(|error| format!("invalid websocket event: {error}"))
            })
            .and_then(|event| {
                if matches!(
                    event["event"]["type"].as_str(),
                    Some("turn_failed" | "error")
                ) {
                    Err(format!("websocket service failed: {event}"))
                } else {
                    Ok(event)
                }
            });
        match result {
            Ok(event) => event,
            Err(error) => {
                // These exact child handles belong to this fixture. Never
                // signal the test runner's shared process group.
                for child in [attached, server] {
                    if child.try_wait().unwrap().is_none() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
                panic!("{error}; recent websocket events: {recent_events:?}");
            }
        }
    };
    let ready = next_event(&mut server, &mut attached);
    assert_eq!(ready["event"]["type"], "ready");
    assert_eq!(ready["event"]["transport"], "websocket");

    let busy = ca_command(home.path())
        .args(["attach", &endpoint, "--auth-file", token.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(busy.status.code(), Some(4));
    assert!(!String::from_utf8_lossy(&busy.stderr).contains(token_text));

    for operation in [
        serde_json::json!({"schema":"ca.serve.v1","id":"start","op":{"type":"start_session"}}),
        serde_json::json!({"schema":"ca.serve.v1","id":"input","op":{"type":"user_input","text":"hello over websocket"}}),
    ] {
        writeln!(attach_input, "{operation}").unwrap();
        attach_input.flush().unwrap();
    }
    let mut sequence = 1_u64;
    let approval = loop {
        let event = next_event(&mut server, &mut attached);
        assert_eq!(event["sequence"], sequence);
        sequence += 1;
        if event["event"]["type"] == "approval_requested" {
            break event;
        }
    };
    assert!(!workspace.join("websocket-approved.txt").exists());
    writeln!(
        attach_input,
        "{}",
        serde_json::json!({
            "schema":"ca.serve.v1",
            "id":"approve",
            "op":{
                "type":"approval_response",
                "request_id":approval["event"]["request_id"],
                "decision":"allow_once"
            }
        })
    )
    .unwrap();
    attach_input.flush().unwrap();

    let mut saw_delta = false;
    loop {
        let event = next_event(&mut server, &mut attached);
        assert_eq!(event["sequence"], sequence);
        sequence += 1;
        if event["event"]["type"] == "runner"
            && event["event"]["event"]["type"] == "assistant_output"
        {
            assert_eq!(event["event"]["event"]["delta"], "websocket-ok");
            saw_delta = true;
        }
        if event["event"]["type"] == "turn_completed" {
            break;
        }
    }
    assert!(saw_delta);
    assert_eq!(
        std::fs::read_to_string(workspace.join("websocket-approved.txt")).unwrap(),
        "approved\n"
    );
    writeln!(
        attach_input,
        "{}",
        serde_json::json!({"schema":"ca.serve.v1","id":"shutdown","op":{"type":"shutdown"}})
    )
    .unwrap();
    attach_input.flush().unwrap();
    loop {
        let event = next_event(&mut server, &mut attached);
        assert_eq!(event["sequence"], sequence);
        sequence += 1;
        if event["event"]["type"] == "goodbye" {
            break;
        }
    }
    drop(attach_input);
    let attached_result = attached.wait_with_output().unwrap();
    assert!(attached_result.status.success());
    assert!(attached_result.stderr.is_empty());

    let reconnect = ca_command(home.path())
        .args(["attach", &endpoint, "--auth-file", token.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reconnect_input = reconnect.stdin.as_ref().unwrap();
    writeln!(
        reconnect_input,
        "{}",
        serde_json::json!({"schema":"ca.serve.v1","id":"status","op":{"type":"status"}})
    )
    .unwrap();
    writeln!(
        reconnect_input,
        "{}",
        serde_json::json!({"schema":"ca.serve.v1","id":"shutdown","op":{"type":"shutdown"}})
    )
    .unwrap();
    reconnect_input.flush().unwrap();
    let reconnect_result = reconnect.wait_with_output().unwrap();
    assert!(
        reconnect_result.status.success(),
        "reconnect failed: status={:?} stderr={} stdout={}",
        reconnect_result.status.code(),
        String::from_utf8_lossy(&reconnect_result.stderr),
        String::from_utf8_lossy(&reconnect_result.stdout)
    );
    assert!(reconnect_result.stderr.is_empty());
    let reconnect_lines = String::from_utf8(reconnect_result.stdout).unwrap();
    let reconnect_events = reconnect_lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(reconnect_events[0]["sequence"], 0);
    assert_eq!(reconnect_events[0]["event"]["transport"], "websocket");
    assert!(reconnect_events
        .iter()
        .any(|event| event["event"]["type"] == "status"));
    assert_eq!(reconnect_events.last().unwrap()["event"]["type"], "goodbye");

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(server.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    let server_result = server.wait_with_output().unwrap();
    assert!(server_result.status.success());
    assert!(server_result.stdout.is_empty());
    let mut remaining_stderr = String::new();
    server_stderr.read_to_string(&mut remaining_stderr).unwrap();
    assert!(!remaining_stderr.contains(token_text));
    assert!(!remaining_stderr.contains(wrong_text));
    assert!(!home.path().join(".local/share/ca/sessions").exists());

    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].contains("hello over websocket"));
    assert!(requests[1].contains("call-ws-write"));
}

#[test]
fn named_profile_doctor_and_model_listing_use_defaults_without_creating_a_file() {
    let home = tempfile::tempdir().unwrap();
    let profile_path = home.path().join(".config/ca/profiles/work.toml");

    let models = ca_command(home.path())
        .args(["--profile", "work", "--list-models"])
        .output()
        .unwrap();
    assert!(models.status.success());
    assert!(!String::from_utf8(models.stdout).unwrap().is_empty());
    assert!(!profile_path.exists());

    let doctor = ca_command(home.path())
        .args(["--profile", "work", "--doctor", "--output-format", "json"])
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(report["profile"], "work");
    assert!(!profile_path.exists());
}

#[test]
fn audit_is_local_deterministic_redacted_and_does_not_create_profile_state() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let profile_path = home.path().join(".config/ca/profiles/work.toml");

    let clean = ca_command(home.path())
        .current_dir(&workspace)
        .args(["audit", "--profile", "work"])
        .output()
        .unwrap();
    assert!(clean.status.success());
    let clean_text = String::from_utf8(clean.stdout).unwrap();
    assert!(clean_text.contains("Agent-harness audit"));
    assert!(clean_text.contains("Status: COMPLETE"));
    assert!(!profile_path.exists());

    let sentinel = "SEC-120-CLI-SENTINEL";
    std::fs::write(
        workspace.join("AGENTS.md"),
        format!("api_key = \"{sentinel}\"\n"),
    )
    .unwrap();
    let risky = ca_command(home.path())
        .current_dir(&workspace)
        .arg("audit")
        .output()
        .unwrap();
    assert_eq!(risky.status.code(), Some(1));
    let risky_text = String::from_utf8(risky.stdout).unwrap();
    assert!(risky_text.contains("AUD-SECRET-001"));
    assert!(!risky_text.contains(sentinel));
    assert!(risky.stderr.is_empty());
}

#[test]
fn bare_headless_uses_normal_provider_but_does_not_start_mcp_or_load_skills() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let skill_dir = home.path().join(".config/ca/skills/hidden-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: hidden-skill\ndescription: BARE-SKILL-SENTINEL\n---\nsecret body",
    )
    .unwrap();
    let mcp_script = write_fake_mcp_server(&workspace);
    let marker = workspace.join("bare-mcp-started");
    let (base_url, provider) = start_fake_provider(vec![text_sse("done")]);
    install_fake_config(home.path(), &base_url, 2);
    install_fake_mcp(home.path(), &mcp_script, &marker);

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args([
            "--profile",
            "bare",
            "--output-format",
            "jsonl",
            "-p",
            "work",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].contains("BARE-SKILL-SENTINEL"));
    assert!(!requests[0].contains("mcp__fake__echo"));
    assert!(!marker.exists());
}

#[test]
fn doctor_is_offline_by_default_and_does_not_rewrite_config() {
    let home = tempfile::tempdir().unwrap();
    install_fake_config(home.path(), "http://127.0.0.1:9/v1", 2);
    enable_policy_only_sandbox(home.path());
    let config_path = home.path().join(".config/ca/config.toml");
    let before = std::fs::read(&config_path).unwrap();

    let output = ca_command(home.path()).arg("--doctor").output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("ca doctor"));
    assert!(stdout.contains("not checked"));
    assert!(!home.path().join(".local/share/ca").exists());
    assert_eq!(std::fs::read(config_path).unwrap(), before);
}

#[test]
fn doctor_missing_config_fails_without_creating_it() {
    let home = tempfile::tempdir().unwrap();

    let output = ca_command(home.path()).arg("--doctor").output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("config.toml is missing"));
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn doctor_connect_uses_one_safe_probe_and_json_output() {
    let home = tempfile::tempdir().unwrap();
    let (base_url, server) = start_fake_provider(vec![text_sse("discarded")]);
    install_fake_config(home.path(), &base_url, 2);
    enable_policy_only_sandbox(home.path());

    let output = ca_command(home.path())
        .args(["--doctor", "--connect", "--output-format", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "ca.doctor.v1");
    assert_eq!(value["ready"], true);
    assert_eq!(value["providers"][0]["connection"], "working");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("Reply with exactly OK"));
    assert!(!String::from_utf8(output.stdout)
        .unwrap()
        .contains("discarded"));
}

#[test]
fn repeatable_headless_skill_activation_is_exact_and_precedes_provider_io() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let skill_dir = home.path().join(".config/ca/skills/deploy");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: deploy\ndescription: Deploy carefully\n---\nSKILL-BODY-CLI",
    )
    .unwrap();
    let (base_url, metadata_server) = start_fake_provider(vec![text_sse("catalog")]);
    install_fake_config(home.path(), &base_url, 2);

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args(["--output-format", "jsonl", "-p", "catalog"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let requests = metadata_server.join().unwrap();
    assert!(requests[0].contains("Deploy carefully"));
    assert!(!requests[0].contains("SKILL-BODY-CLI"));

    let (base_url, server) = start_fake_provider(vec![text_sse("done")]);
    install_fake_config(home.path(), &base_url, 2);

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args([
            "--output-format",
            "jsonl",
            "--skill",
            "deploy",
            "-p",
            "work",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("SKILL-BODY-CLI"));

    let missing_home = tempfile::tempdir().unwrap();
    install_fake_config(missing_home.path(), "http://127.0.0.1:9/v1", 2);
    let output = ca_command(missing_home.path())
        .current_dir(missing_home.path())
        .args([
            "--output-format",
            "jsonl",
            "--skill",
            "missing",
            "-p",
            "work",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("unknown skill ID"));
}

#[test]
fn headless_mcp_is_lazy_namespaced_executes_and_cleans_up() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let script = write_fake_mcp_server(&workspace);
    let marker = workspace.join("mcp-closed");
    let (base_url, provider) = start_fake_provider(vec![
        tool_sse(
            "search_mcp_tools",
            "mcp-search-1",
            serde_json::json!({"query":"echo", "server":"fake", "limit":1}),
        ),
        tool_sse(
            "mcp__fake__echo",
            "mcp-call-1",
            serde_json::json!({"text":"from-mcp"}),
        ),
        text_sse("finished"),
    ]);
    install_fake_config(home.path(), &base_url, 4);
    #[cfg(not(target_os = "macos"))]
    enable_policy_only_sandbox(home.path());
    install_fake_mcp(home.path(), &script, &marker);
    assert!(!marker.exists());

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args(["--output-format", "jsonl", "--yolo", "-p", "use mcp"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout
        .lines()
        .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()));
    assert!(stdout.contains("mcp__fake__echo"));
    assert!(stdout.contains("from-mcp"));
    assert!(!stdout.contains("resolved-secret"));
    let requests = provider.join().unwrap();
    assert!(requests[0].contains("search_mcp_tools"));
    assert!(!requests[0].contains("mcp__fake__echo"));
    assert!(requests[1].contains("mcp__fake__echo"));
    assert!(requests[2].contains("from-mcp"));
    assert!(marker.exists(), "MCP child must close before process exit");
}

#[test]
fn restrictive_tool_base_cannot_be_bypassed_by_mcp_catalog() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let script = write_fake_mcp_server(&workspace);
    let marker = workspace.join("mcp-filter-closed");
    let (base_url, provider) = start_fake_provider(vec![text_sse("finished")]);
    install_fake_config(home.path(), &base_url, 2);
    install_fake_mcp(home.path(), &script, &marker);

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args([
            "--output-format",
            "jsonl",
            "--tools",
            "read_file",
            "-p",
            "do not use mcp",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 1);
    let body = requests[0].split_once("\r\n\r\n").unwrap().1;
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    let names = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["read_file"]);
    assert!(!marker.exists(), "restrictive selection must not start MCP");
}

#[test]
fn headless_mcp_prompts_by_default_and_exits_four_without_calling_server_tool() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let script = write_fake_mcp_server(&workspace);
    let marker = workspace.join("mcp-approval-closed");
    let (base_url, provider) = start_fake_provider(vec![tool_sse(
        "search_mcp_tools",
        "mcp-search-approval",
        serde_json::json!({"query":"echo", "server":"fake"}),
    )]);
    install_fake_config(home.path(), &base_url, 2);
    install_fake_mcp(home.path(), &script, &marker);

    let output = ca_command(home.path())
        .current_dir(&workspace)
        .args(["--output-format", "jsonl", "-p", "use mcp"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("approval_required"));
    assert!(stdout.contains("search_mcp_tools"));
    assert!(!stdout.contains("tool_finish"));
    assert_eq!(provider.join().unwrap().len(), 1);
    assert!(!marker.exists(), "unapproved search must not start MCP");
}

#[test]
fn version_is_local_and_successful() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .arg("--version")
        .output()
        .expect("run ca --version");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        concat!("ca ", env!("CARGO_PKG_VERSION"))
    );
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn list_models_prints_configured_models_and_exits_zero() {
    let home = tempfile::tempdir().expect("temporary home");
    install_fake_config(home.path(), "http://127.0.0.1:1/v1", 100);
    let output = ca_command(home.path())
        .arg("--list-models")
        .output()
        .expect("run ca --list-models");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fake"), "should list fake provider");
    assert!(stdout.contains("fake-model"), "should list fake-model");
    assert!(stdout.contains("(default)"), "should mark default model");
}

#[test]
fn list_models_with_empty_config_prints_notice() {
    let home = tempfile::tempdir().expect("temporary home");
    let config_dir = home.path().join(".config/ca");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[defaults]\nprovider = \"p\"\nmodel = \"m\"\n",
    )
    .unwrap();
    let output = ca_command(home.path())
        .arg("--list-models")
        .output()
        .expect("run ca --list-models empty");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no models configured"));
}

#[test]
fn unknown_option_is_rejected_without_runtime_state() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .arg("--definitely-unknown")
        .output()
        .expect("run ca with unknown option");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn help_lists_structured_headless_contract_without_runtime_state() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .arg("--help")
        .output()
        .expect("run ca --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in [
        "--output-format",
        "--show-thinking",
        "--auto-review",
        "--yolo",
        "--allow-network",
        "--output-last-message",
        "--list-models",
        "--tools",
        "--include-tools",
        "--exclude-tools",
        "--check",
        "--check-repair",
        "--output-schema",
    ] {
        assert!(stdout.contains(flag), "missing {flag} in help");
    }
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn auto_review_and_yolo_are_mutually_exclusive_before_runtime_state() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .args(["--auto-review", "--yolo", "-p", "hello"])
        .output()
        .expect("run conflicting headless flags");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    assert!(!home.path().join(".config/ca/config.toml").exists());
}

#[test]
fn empty_headless_input_exits_two() {
    let home = tempfile::tempdir().expect("temporary home");
    let output = ca_command(home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run ca with empty stdin");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let output = ca_command(home.path())
        .args(["--prompt", "   "])
        .output()
        .expect("run ca with whitespace prompt");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn loopback_jsonl_output_is_clean_and_stream_json_is_an_alias() {
    for format in ["jsonl", "stream-json"] {
        let home = tempfile::tempdir().unwrap();
        let (base_url, server) = start_fake_provider(vec![text_sse("hello")]);
        install_fake_config(home.path(), &base_url, 3);

        let output = ca_command(home.path())
            .current_dir(home.path())
            .args(["--output-format", format, "-p", "say hello"])
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let lines: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.first().unwrap()["event"]["type"], "run_start");
        assert_eq!(lines.last().unwrap()["event"]["type"], "completion");
        assert!(lines
            .iter()
            .enumerate()
            .all(|(index, line)| line["sequence"] == index));
        let run_id = lines[0]["run_id"].clone();
        assert!(lines.iter().all(|line| line["run_id"] == run_id));
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("POST /v1/chat/completions"));
    }
}

#[test]
fn loopback_aggregate_json_writes_only_the_last_message_file() {
    let home = tempfile::tempdir().unwrap();
    let last = home.path().join("last.txt");
    let (base_url, server) = start_fake_provider(vec![text_sse("final answer")]);
    install_fake_config(home.path(), &base_url, 3);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args([
            "--output-format",
            "json",
            "--output-last-message",
            last.to_str().unwrap(),
            "-p",
            "answer",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let aggregate: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(aggregate["schema"], "ca.headless.v1");
    assert_eq!(aggregate["success"], true);
    assert_eq!(aggregate["final_message"], "final answer");
    assert_eq!(std::fs::read_to_string(last).unwrap(), "final answer");
    server.join().unwrap();
}

#[test]
fn loopback_check_uses_v2_fresh_report_and_distinct_failed_exit() {
    for (passed, expected) in [(true, 0), (false, 9)] {
        let home = tempfile::tempdir().unwrap();
        let (base_url, server) = start_fake_provider(vec![
            text_sse("main result"),
            tool_sse(
                "report_verification",
                "check-1",
                serde_json::json!({
                    "passed": passed,
                    "summary": if passed { "verified" } else { "missing evidence" },
                    "issues": if passed { vec![] } else { vec!["no test"] },
                    "evidence": []
                }),
            ),
        ]);
        install_fake_config(home.path(), &base_url, 3);

        let output = ca_command(home.path())
            .current_dir(home.path())
            .args(["--output-format", "json", "--check", "-p", "do the task"])
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(expected));
        assert!(output.stderr.is_empty());
        let aggregate: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(aggregate["schema"], "ca.headless.v2");
        assert_eq!(aggregate["verification"]["report"]["passed"], passed);
        assert_eq!(aggregate["final_message"], "main result");
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        let check_body = requests[1].split("\r\n\r\n").nth(1).unwrap();
        let check: serde_json::Value = serde_json::from_str(check_body).unwrap();
        assert_eq!(check["messages"].as_array().unwrap().len(), 2);
        assert!(check.to_string().contains("do the task"));
        assert!(check.to_string().contains("main result"));
        assert!(!check.to_string().contains("write_file"));
    }
}

#[test]
fn loopback_output_schema_is_v2_and_invalid_schema_fails_before_traffic() {
    let home = tempfile::tempdir().unwrap();
    let schema = home.path().join("schema.json");
    std::fs::write(
        &schema,
        r#"{"type":"object","additionalProperties":false,"required":["answer"],"properties":{"answer":{"type":"integer"}}}"#,
    )
    .unwrap();
    let last = home.path().join("last.json");
    let (base_url, server) = start_fake_provider(vec![tool_sse(
        "report_structured_output",
        "structured-1",
        serde_json::json!({"value":{"answer":42}}),
    )]);
    install_fake_config(home.path(), &base_url, 3);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args([
            "--output-format",
            "json",
            "--output-schema",
            schema.to_str().unwrap(),
            "--output-last-message",
            last.to_str().unwrap(),
            "-p",
            "answer",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let aggregate: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(aggregate["schema"], "ca.headless.v2");
    assert_eq!(
        aggregate["structured_output"],
        serde_json::json!({"answer":42})
    );
    assert_eq!(std::fs::read_to_string(last).unwrap(), r#"{"answer":42}"#);
    assert_eq!(server.join().unwrap().len(), 1);

    let invalid_home = tempfile::tempdir().unwrap();
    let invalid = invalid_home.path().join("invalid.json");
    std::fs::write(
        &invalid,
        r#"{"type":"object","properties":{"x":{"$ref":"https://example.invalid/schema"}}}"#,
    )
    .unwrap();
    install_fake_config(invalid_home.path(), "http://127.0.0.1:1/v1", 3);
    let output = ca_command(invalid_home.path())
        .args([
            "--output-format",
            "json",
            "--output-schema",
            invalid.to_str().unwrap(),
            "-p",
            "answer",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let aggregate: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(aggregate["schema"], "ca.headless.v2");
    assert!(aggregate["events"]
        .to_string()
        .contains("remote references"));
}

#[test]
fn loopback_unapproved_mutation_exits_four_without_writing() {
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("blocked.txt");
    let (base_url, server) = start_fake_provider(vec![tool_sse(
        "write_file",
        "call-1",
        serde_json::json!({"path": target, "content":"blocked"}),
    )]);
    install_fake_config(home.path(), &base_url, 3);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args(["--output-format", "jsonl", "-p", "write it"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stderr.is_empty());
    assert!(!target.exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("approval_required"));
    assert!(stdout.contains("\"exit_code\":4"));
    server.join().unwrap();
}

#[test]
fn loopback_yolo_tool_round_emits_typed_events_and_completes() {
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("written.txt");
    let (base_url, server) = start_fake_provider(vec![
        tool_sse(
            "write_file",
            "call-1",
            serde_json::json!({"path": target, "content":"written"}),
        ),
        text_sse("complete"),
        tool_sse(
            "report_structured_output",
            "review-result",
            serde_json::json!({"value": {
                "approved": true,
                "findings": [],
                "summary": "Exact changed file reviewed."
            }}),
        ),
        text_sse("complete after independent review"),
    ]);
    install_fake_config(home.path(), &base_url, 3);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args(["--output-format", "jsonl", "--yolo", "-p", "write it"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "headless output: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "written");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("tool_start"));
    assert!(stdout.contains("tool_finish"));
    assert!(stdout.contains("\"status\":\"success\""));
    assert!(stdout.contains("completion"));
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests[1].contains("\"role\":\"tool\""));
    let review: serde_json::Value =
        serde_json::from_str(requests[2].split_once("\r\n\r\n").unwrap().1).unwrap();
    let review_tools = review["tools"].as_array().unwrap();
    let review_tool_names = review_tools
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        review_tool_names,
        std::collections::BTreeSet::from([
            "read_file",
            "list_dir",
            "glob",
            "search_files",
            "report_structured_output",
        ])
    );
    assert!(review["messages"].to_string().contains("written.txt"));
    assert!(requests[3].contains("Host-owned independent review"));
    assert!(stdout.contains("complete after independent review"));
}

#[cfg(target_os = "macos")]
#[test]
fn headless_native_candidate_verification_keeps_jsonl_clean_and_completes() {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::time::{Duration, Instant};

    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"native_stdio_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"native_stdio_fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(workspace.join(".gitignore"), "/target/\n").unwrap();
    std::fs::write(
        workspace.join("src/lib.rs"),
        "pub fn answer() -> u32 { 0 }\n",
    )
    .unwrap();
    for arguments in [vec!["init", "--quiet"], vec!["add", "."]] {
        assert!(Command::new("git")
            .args(arguments)
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success());
    }
    assert!(Command::new("git")
        .args([
            "-c",
            "user.name=NativeFixture",
            "-c",
            "user.email=native-fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "baseline",
        ])
        .current_dir(&workspace)
        .status()
        .unwrap()
        .success());
    let source = "pub fn answer() -> u32 { 42 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn answer_is_correct() { assert_eq!(super::answer(), 42); }\n}\n";
    let (base_url, server) = start_fake_provider(vec![
        tool_sse(
            "write_file",
            "source-write",
            serde_json::json!({"path":"src/lib.rs", "content":source}),
        ),
        text_sse("implemented and tested"),
        tool_sse(
            "report_structured_output",
            "review-result",
            serde_json::json!({"value": {
                "approved": true,
                "findings": [],
                "summary": "Exact final source and regression reviewed."
            }}),
        ),
        text_sse("completed after native checks and independent review"),
    ]);
    install_fake_config(home.path(), &base_url, 5);
    let mut command = ca_command(home.path());
    command
        .current_dir(&workspace)
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("XDG_DATA_HOME", home.path().join(".local/share"))
        .env("XDG_CACHE_HOME", home.path().join(".cache"))
        .env("TMPDIR", home.path())
        .args([
            "--workspace",
            workspace.to_str().unwrap(),
            "--trust",
            "--yolo",
            "--output-format",
            "jsonl",
            "--prompt",
            "Fix the answer function and add a boundary regression.",
        ])
        .process_group(0)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(45) {
            timed_out = true;
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_reader.join().unwrap();
    let stderr = stderr_reader.join().unwrap();
    assert!(
        !timed_out,
        "headless native verification exceeded its bounded deadline"
    );
    assert_eq!(
        status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr),
    );
    assert!(
        stderr.is_empty(),
        "embedded native progress leaked onto stderr"
    );
    let events: Vec<serde_json::Value> = String::from_utf8(stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).expect("non-JSON output contaminated JSONL"))
        .collect();
    let completions: Vec<_> = events
        .iter()
        .filter(|event| event["event"]["type"] == "completion")
        .collect();
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0]["event"]["message"],
        "completed after native checks and independent review"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("src/lib.rs")).unwrap(),
        source
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 4);
    let parent: serde_json::Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    let context = parent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|message| message["content"].as_str())
        .find(|content| content.contains("Host-produced automatic verification evidence follows."))
        .expect("parent did not receive native verification evidence");
    let json_start = context.find('{').unwrap();
    let receipt: serde_json::Value = serde_json::from_str(&context[json_start..]).unwrap();
    assert_eq!(receipt["backend"], "native_codex_team");
    assert_eq!(receipt["status"], "completed");
    assert!(!receipt["receipt"]["action_results"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(requests[2].contains("Independently review the exact turn-owned candidate"));
    assert!(requests[2].contains("src/lib.rs"));
    assert!(requests[3].contains("Host-owned independent review"));
}

#[test]
fn loopback_malformed_stream_exits_six_with_structured_failure() {
    let home = tempfile::tempdir().unwrap();
    let (base_url, server) = start_fake_provider(vec!["data: {bad}\n\n".into()]);
    install_fake_config(home.path(), &base_url, 3);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args(["--output-format", "jsonl", "-p", "fail"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(6));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("provider_protocol"));
    assert!(stdout.contains("\"exit_code\":6"));
    server.join().unwrap();
}

#[test]
fn setup_and_transport_failures_use_distinct_structured_exit_codes() {
    let invalid_home = tempfile::tempdir().unwrap();
    let config_dir = invalid_home.path().join(".config/ca");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.toml"), "not valid = [").unwrap();
    let output = ca_command(invalid_home.path())
        .args(["--output-format", "jsonl", "-p", "hello"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("\"exit_code\":3"));

    let transport_home = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    install_fake_config(transport_home.path(), &format!("http://{address}/v1"), 3);
    let output = ca_command(transport_home.path())
        .args(["--output-format", "jsonl", "-p", "hello"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("\"exit_code\":5"));
}

#[test]
fn loopback_max_turn_and_last_message_failures_use_seven_and_eight() {
    let max_home = tempfile::tempdir().unwrap();
    std::fs::write(max_home.path().join("existing.txt"), "content").unwrap();
    let (base_url, server) = start_fake_provider(vec![tool_sse(
        "read_file",
        "call-1",
        serde_json::json!({"path":"existing.txt"}),
    )]);
    install_fake_config(max_home.path(), &base_url, 1);
    let output = ca_command(max_home.path())
        .current_dir(max_home.path())
        .args(["--output-format", "jsonl", "--auto-review", "-p", "read"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("\"exit_code\":7"));
    server.join().unwrap();

    let internal_home = tempfile::tempdir().unwrap();
    let (base_url, server) = start_fake_provider(vec![text_sse("answer")]);
    install_fake_config(internal_home.path(), &base_url, 1);
    let missing = internal_home.path().join("missing/last.txt");
    let output = ca_command(internal_home.path())
        .current_dir(internal_home.path())
        .args([
            "--output-format",
            "jsonl",
            "--output-last-message",
            missing.to_str().unwrap(),
            "-p",
            "answer",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(8));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("\"exit_code\":8"));
    server.join().unwrap();
}

#[test]
fn loopback_default_text_keeps_assistant_on_stdout() {
    let home = tempfile::tempdir().unwrap();
    let (base_url, server) = start_fake_provider(vec![text_sse("plain text")]);
    install_fake_config(home.path(), &base_url, 1);

    let output = ca_command(home.path())
        .current_dir(home.path())
        .args(["-p", "answer"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "plain text\n");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.join().unwrap();
}

#[cfg(unix)]
#[test]
fn sigint_emits_structured_failure_and_exits_130() {
    use std::io::Read;
    let home = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).unwrap();
        accepted_tx.send(()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
    });
    install_fake_config(home.path(), &format!("http://{address}/v1"), 3);
    let child = ca_command(home.path())
        .current_dir(home.path())
        .args(["--output-format", "jsonl", "-p", "wait"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    accepted_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(signal.success());
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(130));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("interrupted"));
    assert!(stdout.contains("\"exit_code\":130"));
    server.join().unwrap();
}
