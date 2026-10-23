//! CA's policy adapter for the shared, authenticated native FreeBSD jail service.
//!
//! The root-owned helper delegates transport and jail lifetime to the existing
//! FreeBSD Codex service. No host command runs when that service is unavailable.
use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const HELPER_PATH: &str = "/usr/local/libexec/codex-freebsd-sandbox-client";
const SOCKET_PATH: &str = "/var/run/codex-freebsd-sandbox/control.sock";
const MAX_POLICY_BYTES: usize = 64 * 1024;

fn require_helper() -> Result<()> {
    let metadata = std::fs::symlink_metadata(HELPER_PATH)
        .context("install the root-owned FreeBSD sandbox client helper")?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == 0
            && metadata.permissions().mode() & 0o022 == 0
            && metadata.permissions().mode() & 0o111 != 0,
        "FreeBSD sandbox helper must be a root-owned, non-writable regular executable"
    );
    Ok(())
}

/// Availability checks touch local metadata only; RPC happens outside the UI.
pub fn available() -> bool {
    require_helper().is_ok()
        && std::fs::metadata(SOCKET_PATH)
            .is_ok_and(|metadata| std::os::unix::fs::FileTypeExt::is_socket(&metadata.file_type()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    version: u32,
    service: String,
    features: Vec<String>,
}

fn validate_capabilities(bytes: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.len() <= MAX_POLICY_BYTES,
        "sandbox capabilities exceed their bound"
    );
    let status: Capabilities = serde_json::from_slice(bytes)?;
    anyhow::ensure!(
        status.version == 1 && status.service == "codex-freebsd-sandboxd",
        "incompatible FreeBSD sandbox service"
    );
    for required in [
        "explicit-filesystem-v1",
        "peer-identity-v1",
        "jail-lifecycle-v1",
        "stdio-pty-v1",
    ] {
        anyhow::ensure!(
            status.features.iter().any(|feature| feature == required),
            "FreeBSD sandbox service lacks required capability: {required}"
        );
    }
    Ok(())
}

/// Called by the existing blocking verification readiness worker, never the UI.
pub fn require_backend() -> Result<()> {
    require_helper()?;
    let mut child = Command::new(HELPER_PATH)
        .arg("capabilities")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(7);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error.into());
                }
                anyhow::bail!("FreeBSD sandbox readiness timed out; execution blocked");
            }
        }
    };
    anyhow::ensure!(
        status.success(),
        "FreeBSD sandbox service is unavailable or incompatible; execution blocked"
    );
    // The trusted helper's capabilities command creates no children or transport
    // threads; exiting closes its sole pipe writer before this bounded read.
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .context("sandbox capability output")?
        .take(MAX_POLICY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    validate_capabilities(&bytes)
}

/// System tools and public runtime data, without any user's home directory.
pub fn system_reads() -> Vec<PathBuf> {
    [
        "/bin",
        "/sbin",
        "/lib",
        "/libexec",
        "/etc",
        "/usr/bin",
        "/usr/sbin",
        "/usr/lib",
        "/usr/libexec",
        "/usr/share",
        "/usr/include",
        "/usr/local/bin",
        "/usr/local/lib",
        "/usr/local/libexec",
        "/usr/local/share",
        "/usr/local/include",
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|path| path.exists())
    .collect()
}

pub fn prepare_process(
    program: &Path,
    arguments: &[String],
    writable: &[PathBuf],
    readable: &[PathBuf],
    denied: &[PathBuf],
    network_allowed: bool,
) -> Result<(PathBuf, Vec<String>)> {
    require_helper()?;
    let permissions = permissions(writable, readable, denied, network_allowed)?;
    let encoded = serde_json::to_string(&permissions)?;
    anyhow::ensure!(
        encoded.len() <= MAX_POLICY_BYTES,
        "FreeBSD sandbox policy exceeds its bound"
    );
    let mut args = vec![
        "run".to_owned(),
        "--permissions-json".to_owned(),
        encoded,
        "--".to_owned(),
        program
            .to_str()
            .context("sandbox executable is not UTF-8")?
            .to_owned(),
    ];
    args.extend_from_slice(arguments);
    Ok((PathBuf::from(HELPER_PATH), args))
}

fn permissions(
    writable: &[PathBuf],
    readable: &[PathBuf],
    denied: &[PathBuf],
    network_allowed: bool,
) -> Result<serde_json::Value> {
    let mut entries = vec![
        serde_json::json!({"path": {"type": "special", "value": {"kind": "minimal"}}, "access": "read"}),
    ];
    for (paths, access) in [(readable, "read"), (writable, "write"), (denied, "deny")] {
        for path in paths {
            anyhow::ensure!(
                path.is_absolute()
                    && path != Path::new("/")
                    && path
                        .to_str()
                        .is_some_and(|text| !text.contains(['\n', '\r', '\0']))
                    && path
                        .components()
                        .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
                "sandbox grants require concrete absolute non-root paths"
            );
            entries.push(
                serde_json::json!({"path": {"type": "path", "path": path}, "access": access}),
            );
        }
    }
    anyhow::ensure!(entries.len() <= 513, "FreeBSD sandbox has too many grants");
    Ok(
        serde_json::json!({"type": "managed", "file_system": {"type": "restricted", "entries": entries},
        "network": if network_allowed { "enabled" } else { "restricted" }}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_keeps_explicit_reads_writes_denials_and_network() {
        let value = permissions(
            &["/work".into()],
            &["/deps".into()],
            &["/work/private".into()],
            false,
        )
        .unwrap();
        assert_eq!(value["network"], "restricted");
        let entries = value["file_system"]["entries"].as_array().unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry["path"]["path"] == "/deps" && entry["access"] == "read"));
        assert!(entries
            .iter()
            .any(|entry| entry["path"]["path"] == "/work" && entry["access"] == "write"));
        assert!(entries
            .iter()
            .any(|entry| entry["path"]["path"] == "/work/private" && entry["access"] == "deny"));
        assert_eq!(
            permissions(&[], &[], &[], true).unwrap()["network"],
            "enabled"
        );
    }

    #[test]
    fn policy_rejects_root_relative_and_escaping_grants() {
        for path in ["/", "relative", "/work/../private"] {
            assert!(permissions(&[path.into()], &[], &[], false).is_err());
        }
    }

    #[test]
    fn capabilities_require_protocol_identity_and_every_enforcement_feature() {
        let valid = serde_json::json!({"version":1,"service":"codex-freebsd-sandboxd",
            "features":["explicit-filesystem-v1","peer-identity-v1","jail-lifecycle-v1","stdio-pty-v1"]});
        assert!(validate_capabilities(&serde_json::to_vec(&valid).unwrap()).is_ok());
        for key in ["version", "service", "features"] {
            let mut invalid = valid.clone();
            invalid[key] = serde_json::Value::Null;
            assert!(validate_capabilities(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        let mut missing = valid;
        missing["features"].as_array_mut().unwrap().pop();
        assert!(validate_capabilities(&serde_json::to_vec(&missing).unwrap()).is_err());
        assert!(validate_capabilities(&vec![b' '; MAX_POLICY_BYTES + 1]).is_err());
    }
}
