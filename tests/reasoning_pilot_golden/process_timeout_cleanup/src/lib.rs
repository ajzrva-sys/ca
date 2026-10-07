use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum RunOutcome {
    Exited(ExitStatus),
    TimedOut,
}

struct OwnedGroup(i32);
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        // The child was placed in this new group before exec. Never derive
        // signal authority from a file written by the program being tested.
        unsafe {
            kill(-self.0, SIGKILL);
        }
    }
}

pub fn run_with_timeout(
    program: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<RunOutcome, String> {
    let mut child = Command::new(program)
        .args(args)
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    let group = OwnedGroup(i32::try_from(child.id()).map_err(|error| error.to_string())?);
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(RunOutcome::Exited(status));
        }
        if Instant::now() >= deadline {
            drop(group);
            child.wait().map_err(|error| error.to_string())?;
            return Ok(RunOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Compatibility with the golden command-line entry point. The same repair
/// also works through the unchanged baseline entry point and public API.
pub fn run_tree_with_timeout(
    program: &Path,
    args: &[String],
    timeout: Duration,
    _pid_file: &Path,
) -> Result<RunOutcome, String> {
    run_with_timeout(program, args, timeout)
}

const SIGKILL: i32 = 9;
extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}
