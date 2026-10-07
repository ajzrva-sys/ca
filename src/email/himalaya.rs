use super::types::{EmailAddress, RecipientSet};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const SUPPORTED_MAJOR: u64 = 2;
const SUPPORTED_MINOR: u64 = 0;
const MAX_VERSION_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum HimalayaError {
    #[error("Himalaya configuration is invalid")]
    InvalidConfiguration,
    #[error("Himalaya v2.0.x is required")]
    UnsupportedVersion,
    #[error("Himalaya could not be started")]
    Spawn,
    #[error("Himalaya operation timed out")]
    Timeout,
    #[error("Himalaya returned too much data")]
    OutputLimit,
    #[error("Himalaya returned an invalid typed result")]
    InvalidOutput,
    #[error("Himalaya operation failed")]
    OperationFailed,
    #[error("email delivery may have occurred; do not retry automatically")]
    DeliveryUnknown,
}

#[derive(Clone)]
pub struct HimalayaAdapter {
    executable: PathBuf,
    config_path: Option<PathBuf>,
    timeout: Duration,
    max_output_bytes: usize,
    version_verified: Arc<tokio::sync::OnceCell<()>>,
}

impl std::fmt::Debug for HimalayaAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HimalayaAdapter")
            .field("executable", &"[CONFIGURED ABSOLUTE EXECUTABLE]")
            .field(
                "config_path",
                &self.config_path.as_ref().map(|_| "[CONFIGURED]"),
            )
            .field("timeout", &self.timeout)
            .field("max_output_bytes", &self.max_output_bytes)
            .finish()
    }
}

impl HimalayaAdapter {
    pub fn new(
        executable: PathBuf,
        config_path: Option<PathBuf>,
        timeout_secs: u64,
        max_output_bytes: usize,
    ) -> Result<Self, HimalayaError> {
        if !executable.is_absolute()
            || config_path.as_ref().is_some_and(|path| !path.is_absolute())
            || !(1..=120).contains(&timeout_secs)
            || !(1024..=1024 * 1024).contains(&max_output_bytes)
        {
            return Err(HimalayaError::InvalidConfiguration);
        }
        Ok(Self {
            executable,
            config_path,
            timeout: Duration::from_secs(timeout_secs),
            max_output_bytes,
            version_verified: Arc::new(tokio::sync::OnceCell::new()),
        })
    }

    pub async fn accounts(&self) -> Result<Value, HimalayaError> {
        self.verify_version().await?;
        let args = self.base_args(&["account", "list"]);
        self.run_json(args, None, false).await
    }

    pub async fn ensure_compatible(&self) -> Result<(), HimalayaError> {
        self.verify_version().await
    }

    pub async fn list(
        &self,
        himalaya_account: &str,
        folder: &str,
        page: u32,
        page_size: u32,
    ) -> Result<Value, HimalayaError> {
        self.verify_version().await?;
        let page = page.to_string();
        let page_size = page_size.to_string();
        let args = self.account_args(
            himalaya_account,
            &[
                "envelope",
                "list",
                "--mailbox",
                folder,
                "--page",
                &page,
                "--page-size",
                &page_size,
            ],
        );
        self.run_json(args, None, false).await
    }

    pub async fn read(
        &self,
        himalaya_account: &str,
        folder: &str,
        message_id: &str,
    ) -> Result<Value, HimalayaError> {
        self.verify_version().await?;
        let args = self.account_args(
            himalaya_account,
            &["message", "read", message_id, "--mailbox", folder],
        );
        self.run_json(args, None, false).await
    }

    pub async fn send(
        &self,
        himalaya_account: &str,
        from: &EmailAddress,
        recipients: &RecipientSet,
        message: Vec<u8>,
    ) -> Result<(), HimalayaError> {
        self.verify_version().await?;
        let mut operation = vec!["smtp".to_string(), "send".to_string()];
        operation.push("--mail-from".into());
        operation.push(from.as_str().into());
        for recipient in recipients.all() {
            operation.push("--rcpt-to".into());
            operation.push(recipient.as_str().into());
        }
        let borrowed = operation.iter().map(String::as_str).collect::<Vec<_>>();
        let args = self.account_args(himalaya_account, &borrowed);
        self.run_json(args, Some(message), true).await.map(|_| ())
    }

    async fn verify_version(&self) -> Result<(), HimalayaError> {
        self.version_verified
            .get_or_try_init(|| async {
                let bytes = self
                    .run_process(vec!["--version".into()], None, MAX_VERSION_BYTES, false)
                    .await?;
                let version =
                    String::from_utf8(bytes).map_err(|_| HimalayaError::UnsupportedVersion)?;
                if !supports_version(&version) {
                    return Err(HimalayaError::UnsupportedVersion);
                }
                Ok(())
            })
            .await
            .copied()
    }

    fn base_args(&self, operation: &[&str]) -> Vec<String> {
        let mut args = vec!["--json".into()];
        if let Some(config_path) = &self.config_path {
            args.push("--config".into());
            args.push(config_path.to_string_lossy().into_owned());
        }
        args.extend(operation.iter().map(|argument| (*argument).to_string()));
        args
    }

    fn account_args(&self, himalaya_account: &str, operation: &[&str]) -> Vec<String> {
        let mut args = self.base_args(&[]);
        args.push("--account".into());
        args.push(himalaya_account.into());
        args.extend(operation.iter().map(|argument| (*argument).to_string()));
        args
    }

    async fn run_json(
        &self,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
        delivery_started_on_spawn: bool,
    ) -> Result<Value, HimalayaError> {
        let bytes = self
            .run_process(
                args,
                stdin,
                self.max_output_bytes,
                delivery_started_on_spawn,
            )
            .await?;
        serde_json::from_slice(&bytes).map_err(|_| {
            if delivery_started_on_spawn {
                HimalayaError::DeliveryUnknown
            } else {
                HimalayaError::InvalidOutput
            }
        })
    }

    async fn run_process(
        &self,
        args: Vec<String>,
        stdin: Option<Vec<u8>>,
        max_output_bytes: usize,
        delivery_started_on_spawn: bool,
    ) -> Result<Vec<u8>, HimalayaError> {
        self.revalidate_paths()?;
        let mut command = tokio::process::Command::new(&self.executable);
        command
            .args(args)
            .env_clear()
            .env("HOME", "/var/empty")
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("RUST_LOG", "off")
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let mut child = command.spawn().map_err(|_| HimalayaError::Spawn)?;
        let process_group = child.id().and_then(|id| i32::try_from(id).ok());
        let mut child_stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_task = tokio::spawn(read_capped(stdout, max_output_bytes));
        let stderr_task = tokio::spawn(read_capped(stderr, 16 * 1024));
        let operation = async {
            if let Some(input) = stdin {
                let Some(mut writer) = child_stdin.take() else {
                    return Err(());
                };
                writer.write_all(&input).await.map_err(|_| ())?;
                writer.shutdown().await.map_err(|_| ())?;
            }
            child.wait().await.map_err(|_| ())
        };
        let status = match tokio::time::timeout(self.timeout, operation).await {
            Ok(Ok(status)) => Some(status),
            Ok(Err(())) => None,
            Err(_) => {
                terminate_process_group(process_group);
                let _ = child.start_kill();
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(if delivery_started_on_spawn {
                    HimalayaError::DeliveryUnknown
                } else {
                    HimalayaError::Timeout
                });
            }
        };
        terminate_process_group(process_group);
        let stdout = stdout_task.await.ok().and_then(Result::ok).ok_or_else(|| {
            delivery_error(delivery_started_on_spawn, HimalayaError::InvalidOutput)
        })?;
        let _stderr = stderr_task.await;
        let Some(status) = status else {
            return Err(delivery_error(
                delivery_started_on_spawn,
                HimalayaError::OperationFailed,
            ));
        };
        if stdout.1 {
            return Err(delivery_error(
                delivery_started_on_spawn,
                HimalayaError::OutputLimit,
            ));
        }
        if !status.success() {
            return Err(delivery_error(
                delivery_started_on_spawn,
                HimalayaError::OperationFailed,
            ));
        }
        Ok(stdout.0)
    }

    fn revalidate_paths(&self) -> Result<(), HimalayaError> {
        validate_regular_absolute(&self.executable, true)?;
        if let Some(config_path) = &self.config_path {
            validate_regular_absolute(config_path, false)?;
        }
        Ok(())
    }
}

fn validate_regular_absolute(path: &Path, executable: bool) -> Result<(), HimalayaError> {
    if !path.is_absolute() {
        return Err(HimalayaError::InvalidConfiguration);
    }
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| HimalayaError::InvalidConfiguration)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HimalayaError::InvalidConfiguration);
    }
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(HimalayaError::InvalidConfiguration);
        }
    }
    Ok(())
}

fn supports_version(raw: &str) -> bool {
    raw.split_whitespace().any(|token| {
        let token = token.trim_start_matches('v');
        let mut parts = token.split('.');
        matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(major), Some(minor), Some(patch))
                if major.parse::<u64>().ok() == Some(SUPPORTED_MAJOR)
                    && minor.parse::<u64>().ok() == Some(SUPPORTED_MINOR)
                    && patch
                        .split(|character: char| !character.is_ascii_digit())
                        .next()
                        .is_some_and(|value| !value.is_empty() && value.parse::<u64>().is_ok())
        )
    })
}

fn delivery_error(started: bool, fallback: HimalayaError) -> HimalayaError {
    if started {
        HimalayaError::DeliveryUnknown
    } else {
        fallback
    }
}

async fn read_capped<R>(reader: Option<R>, cap: usize) -> std::io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let Some(mut reader) = reader else {
        return Ok((Vec::new(), false));
    };
    let mut output = Vec::new();
    let mut overflow = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let available = cap.saturating_sub(output.len());
        let retained = available.min(read);
        output.extend_from_slice(&buffer[..retained]);
        overflow |= retained < read;
    }
    Ok((output, overflow))
}

#[cfg(unix)]
fn terminate_process_group(process_group: Option<i32>) {
    if let Some(process_group) = process_group {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_process_group: Option<i32>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake(root: &Path, version: &str) -> (HimalayaAdapter, PathBuf, PathBuf) {
        let executable = root.join("fake-himalaya");
        let log = root.join("argv.log");
        let input = root.join("stdin.eml");
        let config = root.join("himalaya.toml");
        fs::write(&config, "[accounts.fake]\n").unwrap();
        let script = format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" >> '{}'\nprintf '%s\\n' \"HOME=${{HOME-}}\" \"PATH=${{PATH-}}\" \"SECRET=${{EMAIL_TEST_SECRET-}}\" >> '{}'\nif [ \"${{1-}}\" = '--version' ]; then printf '%s\\n' 'himalaya {version}'; exit 0; fi\ncase \" $* \" in\n  *' account list '*) printf '%s\\n' '{{\"accounts\":[{{\"name\":\"fake\",\"default\":true,\"backends\":[\"imap\",\"smtp\"]}}]}}' ;;\n  *' envelope list '*) printf '%s\\n' '{{\"envelopes\":[]}}' ;;\n  *' message read '*) printf '%s\\n' '{{\"headers\":[],\"body\":\"hello\"}}' ;;\n  *' smtp send '*) /bin/cat > '{}'; printf '%s\\n' '\"Message successfully sent\"' ;;\n  *) exit 9 ;;\nesac\n",
            log.display(),
            log.display(),
            input.display()
        );
        fs::write(&executable, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let adapter = HimalayaAdapter::new(executable, Some(config.clone()), 2, 64 * 1024).unwrap();
        (adapter, log, input)
    }

    #[tokio::test]
    async fn exact_v2_argv_stdin_and_environment_are_closed() {
        let root = tempfile::tempdir().unwrap();
        let (adapter, log, input) = fake(root.path(), "2.0.0");
        std::env::set_var("EMAIL_TEST_SECRET", "must-not-inherit");
        adapter.accounts().await.unwrap();
        adapter.list("fake", "INBOX", 2, 10).await.unwrap();
        adapter.read("fake", "INBOX", "42").await.unwrap();
        adapter
            .send(
                "fake",
                &EmailAddress::parse("me@example.com").unwrap(),
                &RecipientSet::parse(
                    &["to@example.com".into()],
                    &["cc@example.com".into()],
                    &["bcc@example.com".into()],
                )
                .unwrap(),
                b"From: me@example.com\r\n\r\nbody\r\n".to_vec(),
            )
            .await
            .unwrap();
        std::env::remove_var("EMAIL_TEST_SECRET");
        let log = fs::read_to_string(log).unwrap();
        assert_eq!(log.matches("--version").count(), 1);
        for required in [
            "--json",
            "--config",
            "account\nlist",
            "--account\nfake\nenvelope\nlist\n--mailbox\nINBOX\n--page\n2\n--page-size\n10",
            "message\nread\n42\n--mailbox\nINBOX",
            "smtp\nsend\n--mail-from\nme@example.com\n--rcpt-to\nto@example.com\n--rcpt-to\ncc@example.com\n--rcpt-to\nbcc@example.com",
            "HOME=/var/empty",
            "PATH=/usr/bin:/bin",
            "SECRET=",
        ] {
            assert!(log.contains(required), "missing {required:?} in {log}");
        }
        assert_eq!(
            fs::read(input).unwrap(),
            b"From: me@example.com\r\n\r\nbody\r\n"
        );
    }

    #[tokio::test]
    async fn unsupported_version_stops_before_operation() {
        let root = tempfile::tempdir().unwrap();
        let (adapter, _, _) = fake(root.path(), "1.2.0");
        assert_eq!(
            adapter.accounts().await.unwrap_err(),
            HimalayaError::UnsupportedVersion
        );
    }

    #[tokio::test]
    async fn every_post_spawn_send_error_is_unknown() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("fake-himalaya");
        let config = root.path().join("config.toml");
        fs::write(&config, "").unwrap();
        fs::write(
            &executable,
            "#!/bin/sh\nif [ \"${1-}\" = '--version' ]; then echo 'himalaya 2.0.0'; else /bin/cat >/dev/null; echo private-error >&2; exit 7; fi\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let adapter = HimalayaAdapter::new(executable, Some(config), 2, 64 * 1024).unwrap();
        let error = adapter
            .send(
                "fake",
                &EmailAddress::parse("me@example.com").unwrap(),
                &RecipientSet::parse(&["to@example.com".into()], &[], &[]).unwrap(),
                b"message".to_vec(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, HimalayaError::DeliveryUnknown);
        assert!(!error.to_string().contains("private-error"));
    }
}
