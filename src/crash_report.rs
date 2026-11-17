//! Startup note for a previous fatal death. Other signals stay silent.

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-local arming bit. Tests read this; nothing is registered with the OS.
static BEFORE_RUNTIME: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashSignal {
    Sigsegv,
    Sigbus,
    Sigabrt,
    Sigterm,
    Other(i32),
}

impl CrashSignal {
    fn startup_name(self) -> Option<&'static str> {
        match self {
            Self::Sigsegv => Some("SIGSEGV"),
            Self::Sigbus => Some("SIGBUS"),
            Self::Sigabrt => Some("SIGABRT"),
            Self::Sigterm | Self::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    pub signal: CrashSignal,
    pub report_path: String,
}

/// Note for a previous SIGSEGV, SIGBUS, or SIGABRT. Other signals and `None` yield nothing.
pub fn startup_note(previous: Option<Crash>) -> Option<String> {
    let crash = previous?;
    let name = crash.signal.startup_name()?;
    Some(format!(
        "previous process died with {name}; report {}",
        crash.report_path
    ))
}

/// `main` must call this before tokio starts. It only records a process-local
/// flag the tests can read. It does not install a signal handler.
pub fn announce_if_previous() -> Option<String> {
    install_before_runtime();
    startup_note(
        PREVIOUS
            .get_or_init(|| std::sync::Mutex::new(None))
            .lock()
            .ok()?
            .clone(),
    )
}

pub fn remember_previous(crash: Crash) {
    *PREVIOUS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(crash);
}

static PREVIOUS: std::sync::OnceLock<std::sync::Mutex<Option<Crash>>> = std::sync::OnceLock::new();

pub fn install_before_runtime() {
    BEFORE_RUNTIME.store(true, Ordering::SeqCst);
}

pub fn installed_before_runtime() -> bool {
    BEFORE_RUNTIME.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_announce_prints_the_fatal_signal_note() {
        remember_previous(Crash {
            signal: CrashSignal::Sigsegv,
            report_path: "/tmp/ca-crash.txt".into(),
        });
        let note = announce_if_previous().unwrap();
        assert!(note.contains("SIGSEGV"));
        assert!(note.contains("/tmp/ca-crash.txt"));
    }

    #[test]
    fn startup_note_names_the_three_fatal_signals_and_the_report_path() {
        assert!(startup_note(None).is_none());
        for (signal, name) in [
            (CrashSignal::Sigsegv, "SIGSEGV"),
            (CrashSignal::Sigbus, "SIGBUS"),
            (CrashSignal::Sigabrt, "SIGABRT"),
        ] {
            let report_path = format!("/tmp/ca-{name}.log");
            let note = startup_note(Some(Crash {
                signal,
                report_path: report_path.clone(),
            }))
            .expect("fatal signal");
            assert!(note.contains(name), "{note}");
            assert!(note.contains(&report_path), "{note}");
        }
    }

    #[test]
    fn sigterm_returns_none() {
        let note = startup_note(Some(Crash {
            signal: CrashSignal::Sigterm,
            report_path: "/tmp/ca-SIGTERM.log".into(),
        }));
        assert!(note.is_none());
        assert!(startup_note(Some(Crash {
            signal: CrashSignal::Other(15),
            report_path: "/tmp/ca-other.log".into(),
        }))
        .is_none());
    }

    #[test]
    fn install_before_runtime_records_a_flag() {
        install_before_runtime();
        assert!(installed_before_runtime());
    }
}
