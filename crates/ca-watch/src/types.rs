use std::fmt;
use std::path::Path;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const DEFAULT_INTERVAL_SECS: u64 = 10;
pub const MIN_INTERVAL_SECS: u64 = 2;
pub const MAX_INTERVAL_SECS: u64 = 300;
pub(crate) const MAX_LABEL_BYTES: usize = 128;
pub(crate) const MAX_DESTINATION_BYTES: usize = 255;
pub(crate) const MAX_PATH_BYTES: usize = 4096;
pub(crate) const MAX_SUMMARY_CHARS: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum HostSpec {
    Local,
    Ssh {
        destination: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
}

impl HostSpec {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Ssh { destination, .. } => destination,
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Ssh { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ProcessSpec {
    #[serde(rename = "process")]
    Generic,
    Copy {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_path: Option<String>,
        completion_marker: String,
    },
}

impl ProcessSpec {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Generic => "process",
            Self::Copy { .. } => "copy",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchRequestV1 {
    pub label: String,
    pub host: HostSpec,
    #[schemars(range(min = 1))]
    pub pid: u32,
    pub process: ProcessSpec,
    #[serde(default = "default_interval_secs")]
    #[schemars(range(min = 2, max = 300))]
    pub interval_secs: u64,
}

const fn default_interval_secs() -> u64 {
    DEFAULT_INTERVAL_SECS
}

impl WatchRequestV1 {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_text("label", &self.label, MAX_LABEL_BYTES, false)?;
        if self.pid == 0 {
            return Err(ValidationError::InvalidPid);
        }
        if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&self.interval_secs) {
            return Err(ValidationError::InvalidInterval {
                value: self.interval_secs,
            });
        }
        if let HostSpec::Ssh { destination, port } = &self.host {
            validate_destination(destination)?;
            if matches!(port, Some(0)) {
                return Err(ValidationError::InvalidPort);
            }
        }
        if let ProcessSpec::Copy {
            log_path,
            completion_marker,
        } = &self.process
        {
            validate_path("completion_marker", completion_marker)?;
            if let Some(path) = log_path {
                validate_path("log_path", path)?;
            }
        }
        Ok(())
    }
}

fn validate_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), ValidationError> {
    if !allow_empty && value.is_empty() {
        return Err(ValidationError::Empty { field });
    }
    if value.len() > max_bytes {
        return Err(ValidationError::TooLong { field, max_bytes });
    }
    if value.chars().any(char::is_control) {
        return Err(ValidationError::ControlCharacter { field });
    }
    Ok(())
}

fn validate_destination(destination: &str) -> Result<(), ValidationError> {
    validate_text("destination", destination, MAX_DESTINATION_BYTES, false)?;
    if destination.starts_with('-')
        || !destination.is_ascii()
        || destination
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !b"._-@:".contains(&byte))
        || destination.matches('@').count() > 1
    {
        return Err(ValidationError::InvalidDestination);
    }
    Ok(())
}

fn validate_path(field: &'static str, value: &str) -> Result<(), ValidationError> {
    validate_text(field, value, MAX_PATH_BYTES, false)?;
    if !Path::new(value).is_absolute() {
        return Err(ValidationError::PathNotAbsolute { field });
    }
    Ok(())
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct WatchId(Uuid);

impl WatchId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for WatchId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::str::FromStr for WatchId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    Starting,
    Running,
    Degraded,
    Unreachable,
    Completed,
    Failed,
    Exited,
    Stopped,
}

impl WatchState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Exited | Self::Stopped
        )
    }
}

impl fmt::Display for WatchState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Degraded => "degraded",
            Self::Unreachable => "unreachable",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Exited => "exited",
            Self::Stopped => "stopped",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WatchEventV1 {
    pub version: u16,
    pub watch_id: WatchId,
    pub sequence: u64,
    pub observed_at: DateTime<Utc>,
    pub state: WatchState,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WatchSnapshotV1 {
    pub version: u16,
    pub watch_id: WatchId,
    pub request: WatchRequestV1,
    pub started_at: DateTime<Utc>,
    pub event: WatchEventV1,
    pub consecutive_transport_failures: u32,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds the {max_bytes}-byte limit")]
    TooLong {
        field: &'static str,
        max_bytes: usize,
    },
    #[error("{field} contains a control character")]
    ControlCharacter { field: &'static str },
    #[error("pid must be nonzero")]
    InvalidPid,
    #[error("poll interval {value} is outside the supported 2-300 second range")]
    InvalidInterval { value: u64 },
    #[error("SSH destination contains unsupported characters")]
    InvalidDestination,
    #[error("SSH port must be nonzero")]
    InvalidPort,
    #[error("{field} must be an absolute path")]
    PathNotAbsolute { field: &'static str },
}

pub(crate) fn bounded_summary(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let trimmed = sanitized.trim();
    let mut chars = trimmed.chars();
    let bounded: String = chars.by_ref().take(MAX_SUMMARY_CHARS).collect();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> WatchRequestV1 {
        WatchRequestV1 {
            label: "copy".to_string(),
            host: HostSpec::Local,
            pid: 42,
            process: ProcessSpec::Copy {
                log_path: Some("/tmp/copy.log".to_string()),
                completion_marker: "/tmp/done".to_string(),
            },
            interval_secs: DEFAULT_INTERVAL_SECS,
        }
    }

    #[test]
    fn validates_pid_interval_destination_and_paths() {
        assert_eq!(request().validate(), Ok(()));

        let mut invalid = request();
        invalid.pid = 0;
        assert_eq!(invalid.validate(), Err(ValidationError::InvalidPid));

        let mut invalid = request();
        invalid.interval_secs = 1;
        assert!(matches!(
            invalid.validate(),
            Err(ValidationError::InvalidInterval { value: 1 })
        ));

        let mut invalid = request();
        invalid.host = HostSpec::Ssh {
            destination: "root@host;touch /tmp/no".to_string(),
            port: None,
        };
        assert_eq!(invalid.validate(), Err(ValidationError::InvalidDestination));

        let mut invalid = request();
        invalid.process = ProcessSpec::Copy {
            log_path: None,
            completion_marker: "relative".to_string(),
        };
        assert!(matches!(
            invalid.validate(),
            Err(ValidationError::PathNotAbsolute {
                field: "completion_marker"
            })
        ));
    }

    #[test]
    fn event_json_is_explicitly_versioned() {
        let event = WatchEventV1 {
            version: 1,
            watch_id: WatchId::new(),
            sequence: 7,
            observed_at: Utc::now(),
            state: WatchState::Running,
            summary: "alive".to_string(),
        };
        let value = serde_json::to_value(event).expect("event serializes");
        assert_eq!(value["version"], 1);
        assert_eq!(value["state"], "running");
        assert_eq!(value["sequence"], 7);
    }

    #[test]
    fn summaries_are_sanitized_and_bounded() {
        let value = format!("start\n{}", "x".repeat(MAX_SUMMARY_CHARS + 20));
        let bounded = bounded_summary(&value);
        assert!(!bounded.contains('\n'));
        assert!(bounded.chars().count() <= MAX_SUMMARY_CHARS + 1);
    }
}
