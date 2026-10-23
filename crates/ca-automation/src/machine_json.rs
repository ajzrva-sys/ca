//! Thread-local machine JSON mode for Hermes / unattended `--json` stdout.
//!
//! When enabled, human progress must go to stderr so stdout stays a single
//! receipt object. Shared so `app` and `verification` print paths agree.

use std::cell::Cell;

thread_local! {
    static MACHINE_JSON_MODE: Cell<bool> = const { Cell::new(false) };
    static PROGRESS_SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) struct MachineJsonModeGuard {
    previous: bool,
}

impl MachineJsonModeGuard {
    pub(crate) fn enter() -> Self {
        Self {
            previous: MACHINE_JSON_MODE.with(|flag| flag.replace(true)),
        }
    }
}

impl Drop for MachineJsonModeGuard {
    fn drop(&mut self) {
        MACHINE_JSON_MODE.with(|flag| flag.set(self.previous));
    }
}

pub(crate) fn enabled() -> bool {
    MACHINE_JSON_MODE.with(|flag| flag.get())
}

/// Embedded native calls return their receipts to the host; they own no terminal.
pub(crate) struct ProgressSuppressionGuard {
    previous: bool,
}

impl ProgressSuppressionGuard {
    pub(crate) fn enter() -> Self {
        Self {
            previous: PROGRESS_SUPPRESSED.with(|flag| flag.replace(true)),
        }
    }
}

impl Drop for ProgressSuppressionGuard {
    fn drop(&mut self) {
        PROGRESS_SUPPRESSED.with(|flag| flag.set(self.previous));
    }
}

pub(crate) fn progress_suppressed() -> bool {
    PROGRESS_SUPPRESSED.with(|flag| flag.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_output_scopes_restore_the_callers_mode() {
        assert!(!enabled());
        assert!(!progress_suppressed());
        {
            let _machine = MachineJsonModeGuard::enter();
            let _silent = ProgressSuppressionGuard::enter();
            assert!(enabled());
            assert!(progress_suppressed());
            {
                let _nested_machine = MachineJsonModeGuard::enter();
                let _nested_silent = ProgressSuppressionGuard::enter();
                assert!(enabled());
                assert!(progress_suppressed());
            }
            assert!(enabled());
            assert!(progress_suppressed());
            let thread = std::thread::spawn(|| (!enabled(), !progress_suppressed()));
            assert_eq!(thread.join().unwrap(), (true, true));
        }
        assert!(!enabled());
        assert!(!progress_suppressed());
    }
}
