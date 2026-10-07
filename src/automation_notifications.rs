//! Best-effort metadata-only alerts. Durable attempt admission is separate from
//! run/session commits; an ambiguous OS delivery is never replayed.
use super::{
    AutomationClaimV1, AutomationNotificationPolicy, DeliveryCheckpointV1, HeartbeatNotificationV1,
    HeartbeatStatus,
};
use crate::runtime::{ActiveProfile, RuntimePaths};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeAlertV1 {
    schema: String,
    run_id: Uuid,
    automation_id: String,
    name: String,
    profile: String,
    outcome: String,
    target_session_id: Option<Uuid>,
    heartbeat: bool,
}

pub(super) fn freeze(
    claim: &AutomationClaimV1,
    heartbeat: Option<&HeartbeatNotificationV1>,
    failed: bool,
    target_session_id: Option<Uuid>,
) -> Option<NativeAlertV1> {
    if claim.definition.notification_policy == AutomationNotificationPolicy::Muted
        || heartbeat.is_some_and(|decision| !decision.publish)
        || (heartbeat.is_none()
            && !failed
            && claim.definition.notification_policy == AutomationNotificationPolicy::FailuresOnly)
    {
        return None;
    }
    let outcome = if failed {
        "failed"
    } else {
        match heartbeat.map(|decision| decision.status) {
            Some(HeartbeatStatus::Unchanged) => "check completed",
            Some(HeartbeatStatus::Changed) => "changed",
            Some(HeartbeatStatus::Completed) | None => "completed",
            Some(HeartbeatStatus::Failed) => "reported failure",
            Some(HeartbeatStatus::NeedsAttention) => "needs attention",
        }
    };
    Some(NativeAlertV1 {
        schema: "ca.automation-native-alert.v1".into(),
        run_id: claim.run_id,
        automation_id: claim.automation_id.clone(),
        name: super::bounded(
            &crate::terminal_text::sanitize_terminal_text(&claim.definition.name),
            96,
        ),
        profile: super::bounded(
            &crate::terminal_text::sanitize_terminal_text(&claim.profile),
            64,
        ),
        outcome: outcome.into(),
        target_session_id,
        heartbeat: matches!(
            claim.definition.automation_kind,
            super::AutomationKindV1::Heartbeat { .. }
        ),
    })
}

pub(super) fn validate(checkpoint: &DeliveryCheckpointV1) -> bool {
    checkpoint.native_alert.as_ref().is_none_or(|alert| {
        Some(alert.clone())
            == freeze(
                &checkpoint.intent.claim,
                checkpoint.heartbeat_notification.as_ref(),
                checkpoint.execution_failed,
                Some(checkpoint.intent.target_session_id),
            )
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AlertAdmission {
    schema: String,
    alert: NativeAlertV1,
    payload_sha256: String,
    admitted_at: chrono::DateTime<chrono::Utc>,
    disposition: String,
}

trait NotificationTransport {
    fn send(&self, alert: &NativeAlertV1) -> std::io::Result<()>;
}

struct SystemNotificationTransport;
impl NotificationTransport for SystemNotificationTransport {
    fn send(&self, alert: &NativeAlertV1) -> std::io::Result<()> {
        // Never emit real OS alerts from fixture validation.
        #[cfg(test)]
        {
            let _ = alert;
            Ok(())
        }
        #[cfg(all(not(test), target_os = "macos"))]
        {
            use std::process::{Command, Stdio};
            let body = format!(
                "{} · {}\nRun {}\n/automation history {}{}",
                alert.profile,
                alert.outcome,
                alert.run_id,
                alert.automation_id,
                alert
                    .target_session_id
                    .map_or_else(String::new, |id| format!("\nSession {id}"))
            );
            let script = "on run argv\ndisplay notification (item 2 of argv) with title \"CA automation\" subtitle (item 1 of argv)\nend run";
            let mut child = Command::new("/usr/bin/osascript")
                .arg("-e")
                .arg(script)
                .arg("--")
                .arg(&alert.name)
                .arg(body)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let status = match child.try_wait() {
                    Ok(status) => status,
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                };
                if let Some(status) = status {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(std::io::Error::other("native alert transport failed"))
                    };
                }
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "native alert transport timed out",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        #[cfg(all(not(test), not(target_os = "macos")))]
        {
            let _ = alert;
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "native automation alerts are unavailable",
            ))
        }
    }
}

fn focus_suppresses(paths: &RuntimePaths, profile: &ActiveProfile, alert: &NativeAlertV1) -> bool {
    let profile_paths = paths.clone().for_profile(profile);
    let Some(config_parent) = profile_paths.config_file.parent() else {
        return true;
    };
    let directory = config_parent.join("dashboard-presence");
    if !directory.exists() {
        return false;
    }
    let Ok(store) = crate::dashboard_presence::PresenceStore::open(directory) else {
        return true;
    };
    let Ok(scan) = store.scan_with_sessions(Uuid::nil(), chrono::Utc::now(), None) else {
        return true;
    };
    let Ok(scope) =
        crate::dashboard_presence::profile_storage_digest(&paths.profile_data_dir(profile))
    else {
        return true;
    };
    scan.focus_unavailable
        || scan.verified_focus.iter().any(|observation| {
            observation.owner.profile_storage_sha256 == scope
                && (observation.focus.ambiguous_modal
                    || (observation.focus.terminal_focused
                        && (!alert.heartbeat
                            || alert.target_session_id.is_none()
                            || observation.focus.session_id == alert.target_session_id)))
        })
}

fn attempt_with(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    alert: &NativeAlertV1,
    suppress: bool,
    transport: &dyn NotificationTransport,
) {
    let root = super::run_directory(paths, profile, alert.run_id);
    let path = root.join("notification.json");
    if path.exists() {
        return;
    }
    let Ok(payload) = serde_json::to_vec(alert) else {
        return;
    };
    let receipt = AlertAdmission {
        schema: "ca.automation-native-alert-admission.v1".into(),
        alert: alert.clone(),
        payload_sha256: super::hex_digest(&Sha256::digest(payload)),
        admitted_at: chrono::Utc::now(),
        disposition: if suppress {
            "suppressed_focus"
        } else {
            "attempt_admitted"
        }
        .into(),
    };
    let Ok(bytes) = serde_json::to_vec(&receipt) else {
        return;
    };
    // If attempt admission cannot be durably stored, leave the committed run
    // untouched and do not risk an alert that could be repeated on restart.
    if super::atomic_owner_write(&root, &path, &bytes, false).is_err() || suppress {
        return;
    }
    let outcome = if transport.send(alert).is_ok() {
        "delivered"
    } else {
        "transport_failed"
    };
    let _ = super::atomic_owner_write(&root, &root.join("notification-outcome.json"),
        serde_json::json!({"schema":"ca.automation-native-alert-outcome.v1","run_id":alert.run_id,"outcome":outcome}).to_string().as_bytes(), false);
}

pub(super) fn attempt(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    alert: Option<&NativeAlertV1>,
) {
    if let Some(alert) = alert {
        attempt_with(
            paths,
            profile,
            alert,
            focus_suppresses(paths, profile, alert),
            &SystemNotificationTransport,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeTransport {
        calls: std::cell::Cell<usize>,
        fail: bool,
    }
    impl NotificationTransport for FakeTransport {
        fn send(&self, _: &NativeAlertV1) -> std::io::Result<()> {
            self.calls.set(self.calls.get() + 1);
            if self.fail {
                Err(std::io::Error::other("fixture failure"))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn durable_alert_admission_suppresses_focus_and_never_replays_ambiguous_delivery() {
        for suppress in [false, true] {
            for fail in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let paths = RuntimePaths::from_home(temp.path().canonicalize().unwrap());
                let profile = ActiveProfile::Default;
                let alert = NativeAlertV1 {
                    schema: "ca.automation-native-alert.v1".into(),
                    run_id: Uuid::new_v4(),
                    automation_id: "fixture".into(),
                    name: "Review".into(),
                    profile: "default".into(),
                    outcome: "completed".into(),
                    target_session_id: None,
                    heartbeat: false,
                };
                super::super::ensure_private_dir(&super::super::run_directory(
                    &paths,
                    &profile,
                    alert.run_id,
                ))
                .unwrap();
                let fake = FakeTransport {
                    calls: std::cell::Cell::new(0),
                    fail,
                };
                attempt_with(&paths, &profile, &alert, suppress, &fake);
                attempt_with(&paths, &profile, &alert, false, &fake);
                assert_eq!(fake.calls.get(), usize::from(!suppress));
                let bytes = super::super::read_owner_file(
                    &super::super::run_directory(&paths, &profile, alert.run_id)
                        .join("notification.json"),
                    4096,
                )
                .unwrap();
                let admission: AlertAdmission = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(admission.alert, alert);
            }
        }
    }

    #[test]
    fn fresh_verified_profile_session_focus_and_ambiguous_ui_suppress_alerts() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path().canonicalize().unwrap());
        let profile = ActiveProfile::Default;
        let scope = paths.profile_data_dir(&profile);
        super::super::ensure_private_dir(&scope).unwrap();
        let config = paths.clone().for_profile(&profile).config_file;
        super::super::ensure_private_dir(config.parent().unwrap()).unwrap();
        let presence = crate::dashboard_presence::PresenceStore::open(
            config.parent().unwrap().join("dashboard-presence"),
        )
        .unwrap();
        let owner = crate::dashboard_presence::PresenceOwner::current_for_scope(&scope).unwrap();
        let session_id = Uuid::new_v4();
        let alert = NativeAlertV1 {
            schema: "ca.automation-native-alert.v1".into(),
            run_id: Uuid::new_v4(),
            automation_id: "fixture".into(),
            name: "Review".into(),
            profile: "default".into(),
            outcome: "completed".into(),
            target_session_id: Some(session_id),
            heartbeat: true,
        };
        let mut focus = crate::dashboard_presence::PresenceFocus {
            session_id: Some(session_id),
            terminal_focused: true,
            ambiguous_modal: false,
        };
        presence.publish(&owner, &[], focus.clone()).unwrap();
        assert!(focus_suppresses(&paths, &profile, &alert));
        focus.session_id = Some(Uuid::new_v4());
        presence.publish(&owner, &[], focus.clone()).unwrap();
        assert!(!focus_suppresses(&paths, &profile, &alert));
        let mut standalone = alert.clone();
        standalone.heartbeat = false;
        assert!(focus_suppresses(&paths, &profile, &standalone));
        focus.terminal_focused = false;
        presence.publish(&owner, &[], focus.clone()).unwrap();
        assert!(!focus_suppresses(&paths, &profile, &alert));
        focus.ambiguous_modal = true;
        presence.publish(&owner, &[], focus).unwrap();
        assert!(focus_suppresses(&paths, &profile, &alert));
    }

    #[test]
    fn failed_durable_alert_admission_never_calls_transport() {
        let temp = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(temp.path().canonicalize().unwrap());
        let alert = NativeAlertV1 {
            schema: "ca.automation-native-alert.v1".into(),
            run_id: Uuid::new_v4(),
            automation_id: "fixture".into(),
            name: "Review".into(),
            profile: "default".into(),
            outcome: "failed".into(),
            target_session_id: None,
            heartbeat: false,
        };
        let fake = FakeTransport {
            calls: std::cell::Cell::new(0),
            fail: false,
        };
        // An unavailable private run directory makes admission fail. No external
        // notification occurs and no run/session status is changed.
        let root = super::super::run_directory(&paths, &ActiveProfile::Default, alert.run_id);
        super::super::ensure_private_dir(root.parent().unwrap()).unwrap();
        std::fs::write(root, b"blocked").unwrap();
        attempt_with(&paths, &ActiveProfile::Default, &alert, false, &fake);
        assert_eq!(fake.calls.get(), 0);
    }
}
