use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn binary() -> std::path::PathBuf {
    let test_binary = std::env::current_exe().expect("locate integration test artifact");
    let dependencies = test_binary.parent().expect("test artifact directory");
    assert_eq!(
        dependencies.file_name(),
        Some(std::ffi::OsStr::new("deps")),
        "integration test must run from its Cargo artifact directory"
    );
    let binary = dependencies
        .parent()
        .expect("Cargo profile directory")
        .join(format!("ca-watch{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "missing Cargo binary artifact {}: build the workspace binary targets before running this integration test",
        binary.display()
    );
    binary
}

#[test]
fn local_text_and_jsonl_reach_terminal_events() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let marker = directory.path().join("complete");
    std::fs::write(&marker, b"done").expect("write marker");

    let text = Command::new(binary())
        .args([
            "watch",
            "local",
            "--pid",
            "4294967295",
            "--kind",
            "copy",
            "--completion-marker",
            marker.to_str().expect("utf8 marker"),
        ])
        .output()
        .expect("run text CLI");
    assert!(
        text.status.success(),
        "{}",
        String::from_utf8_lossy(&text.stderr)
    );
    let text = String::from_utf8(text.stdout).expect("utf8 text");
    assert!(text.contains("starting"));
    assert!(text.contains("completed"));

    let jsonl = Command::new(binary())
        .args([
            "watch",
            "local",
            "--pid",
            "4294967295",
            "--kind",
            "copy",
            "--completion-marker",
            marker.to_str().expect("utf8 marker"),
            "--jsonl",
        ])
        .output()
        .expect("run JSONL CLI");
    assert!(jsonl.status.success());
    let lines = String::from_utf8(jsonl.stdout).expect("utf8 JSONL");
    let events = lines
        .lines()
        .map(|line| serde_json::from_str::<ca_watch::WatchEventV1>(line).expect("event JSON"))
        .collect::<Vec<_>>();
    assert_eq!(events.first().expect("starting").version, 1);
    assert_eq!(
        events.last().expect("completed").state,
        ca_watch::WatchState::Completed
    );
}

#[test]
fn invalid_arguments_exit_two_without_starting_a_watch() {
    let output = Command::new(binary())
        .args(["watch", "local", "--pid", "0", "--kind", "process"])
        .output()
        .expect("run invalid CLI");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("pid must be nonzero"));

    let output = Command::new(binary())
        .args([
            "watch",
            "ssh",
            "--destination",
            "-oProxyCommand=no",
            "--pid",
            "1",
        ])
        .output()
        .expect("run hostile destination CLI");
    assert!(!output.status.success());
}

#[cfg(unix)]
#[test]
fn ctrl_c_stops_the_observer_and_leaves_the_target_alive() {
    let mut child = Command::new(binary())
        .args([
            "watch",
            "local",
            "--pid",
            &std::process::id().to_string(),
            "--kind",
            "process",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn watcher");
    let mut first_line = String::new();
    BufReader::new(child.stdout.take().expect("watch stdout"))
        .read_line(&mut first_line)
        .expect("read starting event");
    assert!(first_line.contains("starting"));
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) };
    assert_eq!(signal_result, 0);
    let status = child.wait().expect("watcher exits");
    assert_eq!(status.code(), Some(130));
    assert!(std::process::id() > 0);
}
