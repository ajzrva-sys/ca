#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

mod support;

#[test]
fn typed_remote_cli_negotiates_bounded_read_only_transmission_rpc() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4_096];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                request.extend_from_slice(&chunk[..read]);
                let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(text.contains("session-get"));
            if index == 1 {
                assert!(text.contains("X-Transmission-Session-Id: fixed-session"));
            }
            let response = if index == 0 {
                "HTTP/1.1 409 Conflict\r\nX-Transmission-Session-Id: fixed-session\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                let body =
                    r#"{"result":"success","arguments":{"version":"4.0.6","rpc-version":17}}"#;
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("remote.toml");
    std::fs::write(
        &config,
        format!(
            "schema = 'codex-team.remote_config.v1'\n[transmission.seedbox]\nurl = 'http://{address}/transmission/rpc'\nprotocol = 'legacy'\ntimeout_seconds = 10\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
    let request = serde_json::json!({
        "schema":"codex-team.remote_automation_request.v1",
        "actions":[{"kind":"query_remote_rpc","endpoint":"seedbox","operation":"session"}]
    });
    let mut child = Command::new(support::codex_team_binary())
        .args([
            "remote",
            "execute",
            "--config",
            config.to_str().unwrap(),
            "--json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["schema"], "codex-team.remote_automation_receipt.v1");
    assert_eq!(receipt["status"], "completed");
    assert_eq!(receipt["action_results"][0]["kind"], "query_remote_rpc");
    let entry = receipt["action_results"][0]["entries"][0].as_str().unwrap();
    assert!(entry.contains("transmission 4.0.6"));
    assert!(entry.contains("transmission_legacy"));
    server.join().unwrap();
}
