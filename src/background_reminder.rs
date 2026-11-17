//! Names background commands and subagents that finished since the last tool
//! result. Each id is reported once. A user kill includes "do not restart it."

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

const REMINDER_HEADER: &str =
    "Since the last tool result, background commands and subagents finished:";

const DO_NOT_RESTART: &str = "do not restart it.";

/// A command or subagent the user can be told about once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundKind {
    Command,
    Subagent,
}

/// One finished background job. `id` is the once-only key, not `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedJob {
    pub id: String,
    pub name: String,
    pub kind: BackgroundKind,
    pub killed_by_user: bool,
}

impl FinishedJob {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: BackgroundKind,
        killed_by_user: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind,
            killed_by_user,
        }
    }
}

/// Text for the next tool result, or `None` when every id was already named.
///
/// `seen` records ids named by this call. A later call with the same ids is empty.
pub fn next_tool_reminder(
    seen: &mut BTreeSet<String>,
    finished: &[FinishedJob],
) -> Result<Option<String>, String> {
    for job in finished {
        if job.id.is_empty() || job.name.is_empty() {
            return Err("background job id and name must not be empty".into());
        }
        if job.id.contains(['\n', '\r']) || job.name.contains(['\n', '\r']) {
            return Err("background job id and name must be single-line".into());
        }
    }
    let mut lines = Vec::new();
    for job in finished {
        if !seen.insert(job.id.clone()) {
            continue;
        }
        lines.push(format_job(job));
    }
    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!("{REMINDER_HEADER}\n{}", lines.join("\n"))))
}

struct ProcessReminder {
    seen: BTreeSet<String>,
    pending: Vec<FinishedJob>,
}

fn process_reminder() -> &'static Mutex<ProcessReminder> {
    static PROCESS: OnceLock<Mutex<ProcessReminder>> = OnceLock::new();
    PROCESS.get_or_init(|| {
        Mutex::new(ProcessReminder {
            seen: BTreeSet::new(),
            pending: Vec::new(),
        })
    })
}

fn job_is_reportable(job: &FinishedJob) -> bool {
    !job.id.is_empty()
        && !job.name.is_empty()
        && !job.id.contains(['\n', '\r'])
        && !job.name.contains(['\n', '\r'])
}

/// Remember a finished command or subagent until the next tool result.
pub fn record_finished(job: FinishedJob) {
    if !job_is_reportable(&job) {
        return;
    }
    let mut guard = process_reminder()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    guard.pending.push(job);
}

/// Text for jobs finished since the previous successful tool result.
/// Each id is named once.
pub fn take_process_reminder() -> Option<String> {
    let mut guard = process_reminder()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let pending = std::mem::take(&mut guard.pending);
    let pending: Vec<FinishedJob> = pending.into_iter().filter(job_is_reportable).collect();
    next_tool_reminder(&mut guard.seen, &pending).unwrap_or(None)
}

fn format_job(job: &FinishedJob) -> String {
    let kind = match job.kind {
        BackgroundKind::Command => "background command",
        BackgroundKind::Subagent => "background subagent",
    };
    if job.killed_by_user {
        format!("The user killed {kind} {}; {DO_NOT_RESTART}", job.name)
    } else {
        format!("{kind} {} finished.", job.name)
    }
}

#[cfg(test)]
mod tests {
    use super::{next_tool_reminder, BackgroundKind, FinishedJob};
    use std::collections::BTreeSet;

    #[test]
    fn finished_jobs_are_named_once_and_a_kill_is_not_restarted() {
        let mut seen = BTreeSet::new();
        let jobs = vec![
            FinishedJob::new("job-1", "build", BackgroundKind::Command, false),
            FinishedJob::new("job-2", "explore", BackgroundKind::Subagent, false),
        ];
        let first = next_tool_reminder(&mut seen, &jobs)
            .expect("jobs")
            .expect("first reminder");
        assert_eq!(first.matches("build").count(), 1);
        assert_eq!(first.matches("explore").count(), 1);
        assert!(first.contains("background command"));
        assert!(first.contains("background subagent"));
        assert!(next_tool_reminder(&mut seen, &jobs)
            .expect("repeat")
            .is_none());

        let killed = [FinishedJob::new(
            "job-3",
            "deploy",
            BackgroundKind::Command,
            true,
        )];
        let note = next_tool_reminder(&mut seen, &killed)
            .expect("kill")
            .expect("kill reminder");
        assert!(note.contains("deploy"));
        assert!(note.contains("do not restart it."));
        assert!(next_tool_reminder(&mut seen, &killed)
            .expect("kill repeat")
            .is_none());
    }
}
