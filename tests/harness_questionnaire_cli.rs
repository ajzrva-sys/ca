//! Real CLI coverage with an empty environment and disposable profile homes.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn command(home: &Path) -> Command {
    let mut command = isolated_command(home);
    command.args(["harness", "questionnaire"]);
    command
}

fn isolated_command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ca"));
    command
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .current_dir(home);
    command
}

fn output(home: &Path, args: &[&str]) -> Output {
    command(home).args(args).output().unwrap()
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "status={:?}, stderr={}, stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn data_dir(home: &Path) -> PathBuf {
    home.join(".local/share/ca")
}

#[test]
fn help_catalog_and_empty_status_are_read_only_and_local() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let help = output(home, &["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for subcommand in [
        "show", "setup", "init", "steering", "run", "watch", "status",
    ] {
        assert!(help.contains(subcommand), "missing {subcommand}");
    }

    let catalog = success(output(home, &["show", "--json"]));
    let questions = catalog.as_array().unwrap();
    assert!(
        questions.len() >= 30,
        "catalog should cover the whole harness"
    );
    let ids: std::collections::BTreeSet<_> = questions
        .iter()
        .map(|question| question["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), questions.len());
    let text = output(home, &[]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    for question in questions {
        assert!(text.contains(question["id"].as_str().unwrap()));
        assert!(text.contains(question["question"].as_str().unwrap()));
    }

    assert_eq!(success(output(home, &["status", "--json"])), Value::Null);
    let status = output(home, &["status"]);
    assert!(status.status.success());
    assert!(String::from_utf8(status.stdout)
        .unwrap()
        .contains("No questionnaire runs yet"));
    let steering = success(output(home, &["steering", "--json"]));
    assert_eq!(steering["content"], Value::Null);
    assert!(!data_dir(home).exists());
    assert!(!home.join(".config/ca").exists());
}

#[test]
fn run_persists_learning_and_named_profiles_are_isolated() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let first = success(output(home, &["run", "--json"]));
    assert_eq!(first["schema"], "ca.harness_questionnaire.v1");
    assert_eq!(first["scanned_outcomes"], 0);
    assert_eq!(first["independent_roots"], 0);
    assert!(first["cohorts"].as_array().unwrap().is_empty());
    let recorded = success(output(home, &["status", "--json"]));
    let mut stored_first = first.clone();
    stored_first["changed"] = Value::Bool(false);
    assert_eq!(stored_first, recorded);
    let second = success(output(home, &["run", "--limit", "1000", "--json"]));
    assert_eq!(second["changed"], false);
    assert_eq!(second["evidence_digest"], first["evidence_digest"]);
    assert_eq!(second["cycle"], first["cycle"]);

    assert_eq!(
        success(output(home, &["--profile", "work", "status", "--json"])),
        Value::Null
    );
    let mut named = success(output(home, &["run", "--profile", "work", "--json"]));
    assert_eq!(named["scanned_outcomes"], 0);
    named["changed"] = Value::Bool(false);
    assert_eq!(success(output(home, &["status", "--json"])), second);
    assert_eq!(
        success(output(home, &["status", "--profile", "work", "--json"])),
        named
    );
    assert!(data_dir(home)
        .join("harness/questionnaire/state.json")
        .is_file());
    assert!(data_dir(home)
        .join("profiles/work/harness/questionnaire/state.json")
        .is_file());
    assert!(!data_dir(home).join("sessions").exists());
    assert!(!home.join(".config/ca/config.toml").exists());
}

#[test]
fn personality_init_preserves_answers_and_named_steering_is_isolated() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let initialized = success(output(home, &["init", "--json"]));
    let path = PathBuf::from(initialized["path"].as_str().unwrap());
    assert!(path.starts_with(home));
    assert!(path.is_file());
    let original = std::fs::read_to_string(&path).unwrap();
    assert!(original.contains("#"));
    let answers = format!("{original}\nUSER PERSONALITY ANSWER: Explain tradeoffs clearly.\n");
    std::fs::write(&path, &answers).unwrap();
    assert_eq!(success(output(home, &["init", "--json"])), initialized);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), answers);
    let steering = success(output(home, &["steering", "--json"]));
    assert_eq!(steering["path"], initialized["path"]);
    assert_eq!(steering["content"], answers);
    assert!(steering["active_content"]
        .as_str()
        .unwrap()
        .contains("USER PERSONALITY ANSWER"));

    let missing_named = success(output(home, &["steering", "--profile", "work", "--json"]));
    assert_ne!(missing_named["path"], initialized["path"]);
    assert_eq!(missing_named["content"], Value::Null);
    let named_path = PathBuf::from(missing_named["path"].as_str().unwrap());
    assert!(!named_path.exists());
    let initialized_named = success(output(home, &["init", "--profile", "work", "--json"]));
    assert_eq!(initialized_named["path"], missing_named["path"]);
    let named_original = std::fs::read_to_string(&named_path).unwrap();
    assert!(!named_original.is_empty());
    assert!(!named_original.contains("USER PERSONALITY ANSWER"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), answers);
    assert!(!data_dir(home).exists());
    assert!(!home.join(".config/ca/config.toml").exists());
}

#[test]
fn watch_is_bounded_and_quiet_when_evidence_is_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let first = success(output(home, &["run", "--json"]));
    let watched = output(
        home,
        &["watch", "--interval", "1", "--cycles", "2", "--json"],
    );
    assert!(
        watched.status.success(),
        "{}",
        String::from_utf8_lossy(&watched.stderr)
    );
    assert!(watched.stderr.is_empty());
    let text = String::from_utf8(watched.stdout).unwrap();
    let reports: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        reports.len(),
        1,
        "first cycle prints; unchanged repeats stay quiet"
    );
    assert_eq!(reports[0]["changed"], false);
    assert_eq!(reports[0]["evidence_digest"], first["evidence_digest"]);
    let latest = success(output(home, &["status", "--json"]));
    assert_eq!(latest["cycle"], first["cycle"]);
}

#[test]
fn invalid_options_fail_and_bare_profile_does_not_write() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    for args in [
        vec!["watch", "--interval", "0"],
        vec!["watch", "--cycles", "0"],
        vec!["run", "--limit", "0"],
        vec!["run", "--limit", "1001"],
        vec!["unknown"],
    ] {
        let result = output(home, &args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(!result.stderr.is_empty());
    }
    for args in [
        vec!["run", "--profile", "../outside"],
        vec!["run", "--profile", "bare"],
        vec!["init", "--profile", "bare"],
    ] {
        let result = output(home, &args);
        assert_eq!(result.status.code(), Some(1), "{args:?}");
        assert!(!result.stderr.is_empty());
    }
    assert!(!data_dir(home).exists());
    assert!(!home.join(".config/ca").exists());
}

fn install_loopback_config(home: &Path, base_url: &str, profile: &str) {
    let root = home.join(".config/ca");
    let path = if profile == "default" {
        root.join("config.toml")
    } else {
        root.join("profiles").join(format!("{profile}.toml"))
    };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!(
            r#"approval_rules = []

[defaults]
provider = "fake"
model = "fake-model"

[agent]
max_turns = 1
approval_mode = "allowlist"
command_timeout_secs = 2
sandbox_mode = "policy-only"

[providers.fake]
base_url = "{base_url}"
api_key = "disposable-test-only"
models = ["fake-model"]
"#
        ),
    )
    .unwrap();
}

fn start_loopback_provider(request_count: usize) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut requests = Vec::new();
        for _ in 0..request_count {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for CA request"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback accept failed: {error}"),
                }
            };
            // Accepted sockets can inherit the listener's nonblocking mode on
            // BSD/macOS. Wait for the complete request under the bounded socket
            // timeout even when accept wins the race with the client's write.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() <= 2 * 1024 * 1024);
                if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            assert!(length <= 2 * 1024 * 1024);
            while request.len() < header_end + length {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            requests
                .push(serde_json::from_slice(&request[header_end..header_end + length]).unwrap());
            let delta = serde_json::json!({
                "choices": [{"delta": {"content": "personality-fixture-ok"}, "finish_reason": null}]
            });
            let finish = serde_json::json!({
                "choices": [{"delta": {}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}
            });
            let body = format!("data: {delta}\n\ndata: {finish}\n\ndata: [DONE]\n\n");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    (format!("http://{address}/v1"), worker)
}

fn headless(home: &Path, profile: &str) -> Output {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = isolated_command(home)
        .args([
            "--profile",
            profile,
            "--output-format",
            "json",
            "-p",
            "say hello",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let result = child.wait_with_output().unwrap();
            panic!(
                "headless CA exceeded fixture deadline: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn editable_personality_reaches_actual_provider_prompt_with_profile_and_bare_isolation() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let initialized = success(output(home, &["init", "--json"]));
    let default_path = PathBuf::from(initialized["path"].as_str().unwrap());
    let template = std::fs::read_to_string(&default_path).unwrap();
    std::fs::write(
        &default_path,
        format!("{template}\n<!-- UNANSWERED-TEMPLATE-CANARY -->\n"),
    )
    .unwrap();
    let (base_url, provider) = start_loopback_provider(4);
    install_loopback_config(home, &base_url, "default");
    install_loopback_config(home, &base_url, "work");
    success(headless(home, "default"));
    std::fs::write(
        &default_path,
        format!("{template}\nPERSONALITY-DEFAULT-CANARY: Explain tradeoffs clearly.\n"),
    )
    .unwrap();
    success(headless(home, "default"));
    let named = success(output(home, &["init", "--profile", "work", "--json"]));
    let named_path = PathBuf::from(named["path"].as_str().unwrap());
    std::fs::write(
        &named_path,
        "# My style\nPERSONALITY-WORK-CANARY: Be concise.\n",
    )
    .unwrap();
    success(headless(home, "work"));
    success(headless(home, "bare"));
    let requests = provider.join().unwrap();
    assert_eq!(requests.len(), 4);
    let prompts: Vec<String> = requests
        .iter()
        .map(|request| {
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "system")
                .map(|message| message["content"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    assert!(!prompts[0].contains("UNANSWERED-TEMPLATE-CANARY"));
    assert!(!prompts[0].contains("User-authored CA personality and behavior"));
    assert!(prompts[1].contains("PERSONALITY-DEFAULT-CANARY"));
    assert!(!prompts[1].contains("PERSONALITY-WORK-CANARY"));
    assert!(prompts[2].contains("PERSONALITY-WORK-CANARY"));
    assert!(!prompts[2].contains("PERSONALITY-DEFAULT-CANARY"));
    assert!(!prompts[3].contains("PERSONALITY-DEFAULT-CANARY"));
    assert!(!prompts[3].contains("PERSONALITY-WORK-CANARY"));
    assert!(!prompts[3].contains("User-authored CA personality and behavior"));
}

#[test]
fn invalid_personality_fails_actual_headless_path_before_provider_traffic() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let initialized = success(output(home, &["init", "--json"]));
    let path = PathBuf::from(initialized["path"].as_str().unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    install_loopback_config(home, &url, "default");
    for (content, expected) in [
        ("<!-- unfinished".to_string(), "unfinished comment"),
        ("a".repeat(32 * 1024 + 1), "32 KiB"),
    ] {
        std::fs::write(&path, content).unwrap();
        let steering = output(home, &["steering", "--json"]);
        assert_eq!(steering.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&steering.stderr).contains(expected));
        let result = headless(home, "default");
        assert!(!result.status.success());
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(text.contains("Could not load CA personality steering"));
        assert!(text.contains(expected));
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}

#[cfg(unix)]
#[test]
fn continuous_watch_exits_gracefully_on_ctrl_c() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let mut child = command(home)
        .args(["watch", "--interval", "300", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send((result, line));
    });
    let first = receiver.recv_timeout(Duration::from_secs(15));
    if first.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let (read_result, line) = first.expect("watch must emit its first cycle promptly");
    assert!(read_result.unwrap() > 0);
    let first: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(first["schema"], "ca.harness_questionnaire.v1");
    reader.join().unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch did not stop after Ctrl-C");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(
        status.success(),
        "Ctrl-C should exit successfully: {status}"
    );
    let latest = success(output(home, &["status", "--json"]));
    assert_eq!(latest["cycle"], first["cycle"]);
}
