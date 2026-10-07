use crate::runtime::{ActiveProfile, RuntimePaths};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaunchdError {
    #[error("email gateway service preview is unsafe")]
    Unsafe,
    #[error("email gateway service operation failed")]
    Operation,
}

pub type GatewayLaunchAgentPreview = crate::schedule_v2::LaunchAgentPreview;
pub use crate::schedule_v2::ServiceAction;

pub fn preview(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    executable: &Path,
    poll_secs: u64,
) -> Result<GatewayLaunchAgentPreview, LaunchdError> {
    if !(1..=3600).contains(&poll_secs) {
        return Err(LaunchdError::Unsafe);
    }
    let home = paths
        .home
        .canonicalize()
        .map_err(|_| LaunchdError::Unsafe)?;
    let executable = executable
        .canonicalize()
        .map_err(|_| LaunchdError::Unsafe)?;
    let metadata = std::fs::symlink_metadata(&executable).map_err(|_| LaunchdError::Unsafe)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(LaunchdError::Unsafe);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(LaunchdError::Unsafe);
        }
    }
    let suffix = hex(&Sha256::digest(profile.label().as_bytes()))[..16].to_owned();
    let label = format!("com.ca.email-gateway.{suffix}");
    let normalized = RuntimePaths::from_home(&home);
    let log_dir = normalized.profile_data_dir(profile).join("logs");
    let path = home
        .join("Library/LaunchAgents")
        .join(format!("{label}.plist"));
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>email</string>\n    <string>--profile</string>\n    <string>{}</string>\n    <string>gateway</string>\n    <string>daemon</string>\n  </array>\n  <key>EnvironmentVariables</key>\n  <dict><key>HOME</key><string>{}</string></dict>\n  <key>RunAtLoad</key><false/>\n  <key>KeepAlive</key><false/>\n  <key>StartInterval</key><integer>{poll_secs}</integer>\n  <key>StandardOutPath</key><string>{}</string>\n  <key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
        escape(&label), escape(&executable.to_string_lossy()), escape(profile.label()), escape(&home.to_string_lossy()),
        escape(&log_dir.join("email-gateway.out.log").to_string_lossy()), escape(&log_dir.join("email-gateway.err.log").to_string_lossy()),
    ).into_bytes();
    lint_plist(&xml)?;
    let digest = hex(&Sha256::digest(&xml));
    Ok(GatewayLaunchAgentPreview {
        label,
        path,
        xml,
        digest,
    })
}

pub fn mutate(
    action: ServiceAction,
    preview: &GatewayLaunchAgentPreview,
    confirmation: Option<&str>,
    launchctl: &Path,
) -> Result<String, LaunchdError> {
    crate::schedule_v2::service_mutation(action, preview, confirmation, launchctl)
        .map_err(|_| LaunchdError::Operation)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn lint_plist(xml: &[u8]) -> Result<(), LaunchdError> {
    let path = Path::new("/usr/bin/plutil");
    if !path.is_file() {
        return Ok(());
    }
    let mut child = std::process::Command::new(path)
        .args(["-lint", "-"])
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| LaunchdError::Unsafe)?;
    child
        .stdin
        .take()
        .ok_or(LaunchdError::Unsafe)?
        .write_all(xml)
        .map_err(|_| LaunchdError::Unsafe)?;
    if child.wait().map_err(|_| LaunchdError::Unsafe)?.success() {
        Ok(())
    } else {
        Err(LaunchdError::Unsafe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_deterministic_secret_free_and_draft_only() {
        let home = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(home.path());
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let one = preview(&paths, &ActiveProfile::Default, &executable, 30).unwrap();
        let two = preview(&paths, &ActiveProfile::Default, &executable, 30).unwrap();
        assert_eq!(one.digest, two.digest);
        let text = String::from_utf8(one.xml).unwrap();
        assert!(text.contains("<string>email</string>"));
        assert!(text.contains("<string>gateway</string>"));
        assert!(text.contains("<string>daemon</string>"));
        assert!(!text.to_ascii_lowercase().contains("password"));
        assert!(!text.contains("smtp"));
    }

    #[cfg(unix)]
    #[test]
    fn fake_launchctl_lifecycle_requires_exact_digest() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(home.path());
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let preview = preview(
            &paths,
            &ActiveProfile::Named("work".into()),
            &executable,
            45,
        )
        .unwrap();
        let fake = home.path().join("fake-launchctl");
        let log = home.path().join("launchctl.log");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(mutate(ServiceAction::Install, &preview, Some("wrong"), &fake).is_err());
        assert!(!preview.path.exists());
        assert_eq!(
            mutate(
                ServiceAction::Install,
                &preview,
                Some(&preview.digest),
                &fake
            )
            .unwrap(),
            "installed"
        );
        assert!(preview.path.exists());
        assert_eq!(
            mutate(
                ServiceAction::Uninstall,
                &preview,
                Some(&preview.digest),
                &fake
            )
            .unwrap(),
            "uninstalled"
        );
        assert!(!preview.path.exists());
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(calls.contains("bootstrap"));
        assert!(calls.contains("bootout"));
    }
}
