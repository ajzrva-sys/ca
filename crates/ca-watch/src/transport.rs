use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::types::{bounded_summary, HostSpec, ProcessSpec, WatchRequestV1};

const LOCAL_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LOG_READ_BYTES: u64 = 65_536;
const MAX_REMOTE_OUTPUT_BYTES: usize = 131_072;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeObservation {
    pub marker_present: bool,
    pub identity: Option<String>,
    pub log_line: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("process probe unavailable")]
pub struct ProbeError;

#[async_trait]
pub trait ProbeTransport: Send + Sync {
    async fn probe(&self, request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProbeTransport;

#[async_trait]
impl ProbeTransport for SystemProbeTransport {
    async fn probe(&self, request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
        match &request.host {
            HostSpec::Local => probe_local(request).await,
            HostSpec::Ssh { destination, port } => probe_ssh(request, destination, *port).await,
        }
    }
}

async fn probe_local(request: &WatchRequestV1) -> Result<ProbeObservation, ProbeError> {
    if let ProcessSpec::Copy {
        completion_marker, ..
    } = &request.process
    {
        match tokio::fs::try_exists(completion_marker).await {
            Ok(true) => {
                return Ok(ProbeObservation {
                    marker_present: true,
                    identity: None,
                    log_line: None,
                });
            }
            Ok(false) => {}
            Err(_) => return Err(ProbeError),
        }
    }

    let mut command = Command::new("/bin/ps");
    command
        .args([
            "-p",
            &request.pid.to_string(),
            "-o",
            "lstart=",
            "-o",
            "command=",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(LOCAL_PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| ProbeError)?
        .map_err(|_| ProbeError)?;
    let identity = if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!value.is_empty()).then_some(value)
    } else {
        None
    };

    let log_line = match (&request.process, identity.is_some()) {
        (
            ProcessSpec::Copy {
                log_path: Some(path),
                ..
            },
            true,
        ) => read_last_nonempty_line(path.clone()).await?,
        _ => None,
    };

    Ok(ProbeObservation {
        marker_present: false,
        identity,
        log_line,
    })
}

async fn read_last_nonempty_line(path: String) -> Result<Option<String>, ProbeError> {
    tokio::task::spawn_blocking(move || {
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ProbeError),
        };
        let length = file.metadata().map_err(|_| ProbeError)?.len();
        let start = length.saturating_sub(MAX_LOG_READ_BYTES);
        file.seek(SeekFrom::Start(start)).map_err(|_| ProbeError)?;
        let mut bytes = Vec::with_capacity((length - start) as usize);
        file.read_to_end(&mut bytes).map_err(|_| ProbeError)?;
        let value = String::from_utf8_lossy(&bytes);
        Ok(value
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(bounded_summary))
    })
    .await
    .map_err(|_| ProbeError)?
}

async fn probe_ssh(
    request: &WatchRequestV1,
    destination: &str,
    port: Option<u16>,
) -> Result<ProbeObservation, ProbeError> {
    let args = ssh_arguments(request, destination, port);
    let mut command = Command::new("/usr/bin/ssh");
    command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SSH_PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| ProbeError)?
        .map_err(|_| ProbeError)?;
    if !output.status.success() || output.stdout.len() > MAX_REMOTE_OUTPUT_BYTES {
        return Err(ProbeError);
    }
    parse_remote_output(&output.stdout)
}

fn parse_remote_output(output: &[u8]) -> Result<ProbeObservation, ProbeError> {
    let fields: Vec<&[u8]> = output.split(|byte| *byte == 0).collect();
    if fields.len() != 5 || !fields[4].is_empty() {
        return Err(ProbeError);
    }
    let marker_present = match fields[0] {
        b"0" => false,
        b"1" => true,
        _ => return Err(ProbeError),
    };
    let alive = match fields[1] {
        b"0" => false,
        b"1" => true,
        _ => return Err(ProbeError),
    };
    let identity = String::from_utf8(fields[2].to_vec()).map_err(|_| ProbeError)?;
    let progress = String::from_utf8(fields[3].to_vec()).map_err(|_| ProbeError)?;
    if alive != !identity.trim().is_empty() {
        return Err(ProbeError);
    }
    Ok(ProbeObservation {
        marker_present,
        identity: alive.then(|| identity.trim().to_string()),
        log_line: (!progress.trim().is_empty()).then(|| bounded_summary(&progress)),
    })
}

fn ssh_arguments(request: &WatchRequestV1, destination: &str, port: Option<u16>) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("-T"),
        OsString::from("-o"),
        OsString::from("BatchMode=yes"),
        OsString::from("-o"),
        OsString::from("ClearAllForwardings=yes"),
        OsString::from("-o"),
        OsString::from("ConnectTimeout=10"),
        OsString::from("-o"),
        OsString::from("ConnectionAttempts=1"),
        OsString::from("-o"),
        OsString::from("PermitLocalCommand=no"),
        OsString::from("-o"),
        OsString::from("RequestTTY=no"),
    ];
    if let Some(port) = port {
        args.push(OsString::from("-p"));
        args.push(OsString::from(port.to_string()));
    }
    args.push(OsString::from("--"));
    args.push(OsString::from(destination));
    args.push(OsString::from(remote_command(request)));
    args
}

const REMOTE_SCRIPT: &str = r#"set -f
pid=$1
kind=$2
marker=$3
log=$4
marker_present=0
if [ "$kind" = copy ] && [ -e "$marker" ]; then marker_present=1; fi
identity=$(/bin/ps -p "$pid" -o lstart= -o command= 2>/dev/null || :)
alive=0
if [ -n "$identity" ]; then alive=1; fi
progress=
if [ "$kind" = copy ] && [ "$alive" = 1 ] && [ -n "$log" ] && [ -r "$log" ]; then
  progress=$(/usr/bin/tail -c 65536 "$log" 2>/dev/null | /usr/bin/awk 'NF { line=$0 } END { print line }')
fi
/usr/bin/printf '%s\0%s\0%s\0%s\0' "$marker_present" "$alive" "$identity" "$progress""#;

fn remote_command(request: &WatchRequestV1) -> String {
    let (marker, log) = match &request.process {
        ProcessSpec::Generic => ("", ""),
        ProcessSpec::Copy {
            log_path,
            completion_marker,
        } => (
            completion_marker.as_str(),
            log_path.as_deref().unwrap_or(""),
        ),
    };
    [
        "/bin/sh".to_string(),
        "-c".to_string(),
        shell_quote(REMOTE_SCRIPT),
        "ca-watch".to_string(),
        shell_quote(&request.pid.to_string()),
        shell_quote(request.process.kind_name()),
        shell_quote(marker),
        shell_quote(log),
    ]
    .join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostSpec, ProcessSpec, WatchRequestV1};

    fn remote_request() -> WatchRequestV1 {
        WatchRequestV1 {
            label: "BSD copy".to_string(),
            host: HostSpec::Ssh {
                destination: "root@bsd".to_string(),
                port: Some(2222),
            },
            pid: 55_347,
            process: ProcessSpec::Copy {
                log_path: Some("/home/a ji/copy's.log".to_string()),
                completion_marker: "/bigmac/seedbox1/.copy-complete".to_string(),
            },
            interval_secs: 10,
        }
    }

    #[test]
    fn ssh_arguments_are_fixed_and_values_are_quoted() {
        let request = remote_request();
        let args = ssh_arguments(&request, "root@bsd", Some(2222));
        let rendered: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            &rendered[..15],
            &[
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ConnectTimeout=10",
                "-o",
                "ConnectionAttempts=1",
                "-o",
                "PermitLocalCommand=no",
                "-o",
                "RequestTTY=no",
                "-p",
                "2222",
            ]
        );
        assert_eq!(rendered[15], "--");
        assert_eq!(rendered[16], "root@bsd");
        assert!(rendered[17].contains("'55347'"));
        assert!(rendered[17].contains("'/home/a ji/copy'\\''s.log'"));
        assert!(!rendered[17].contains("BatchMode"));
    }

    #[test]
    fn parses_bounded_remote_protocol_without_identity_leakage() {
        let parsed = parse_remote_output(b"0\x001\x00identity bytes\x00item 53 of 993\x00")
            .expect("valid protocol");
        assert_eq!(parsed.identity.as_deref(), Some("identity bytes"));
        assert_eq!(parsed.log_line.as_deref(), Some("item 53 of 993"));
        assert!(parse_remote_output(b"0\x000\x00identity\x00\x00").is_err());
    }

    #[tokio::test]
    async fn local_log_reads_only_the_bounded_tail_and_last_nonempty_line() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("copy.log");
        let mut content = "x".repeat(MAX_LOG_READ_BYTES as usize + 4096);
        content.push_str("\n\nitem 53 of 993\n\n");
        std::fs::write(&path, content).expect("write log");
        let line = read_last_nonempty_line(path.to_string_lossy().into_owned())
            .await
            .expect("read log");
        assert_eq!(line.as_deref(), Some("item 53 of 993"));
    }
}
