//! Thread-local machine JSON mode for Hermes / unattended `--json` stdout.
//!
//! When enabled, human progress must go to stderr so stdout stays a single
//! receipt object. Shared so `app` and `verification` print paths agree.

use std::cell::Cell;

thread_local! {
    static MACHINE_JSON_MODE: Cell<bool> = const { Cell::new(false) };
}

pub(crate) struct MachineJsonModeGuard;

impl MachineJsonModeGuard {
    pub(crate) fn enter() -> Self {
        MACHINE_JSON_MODE.with(|flag| flag.set(true));
        Self
    }
}

impl Drop for MachineJsonModeGuard {
    fn drop(&mut self) {
        MACHINE_JSON_MODE.with(|flag| flag.set(false));
    }
}

pub(crate) fn enabled() -> bool {
    MACHINE_JSON_MODE.with(|flag| flag.get())
}
