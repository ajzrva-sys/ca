use std::path::Path;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum RunOutcome {
    Exited(ExitStatus),
    TimedOut,
}

pub fn run_with_timeout(
    program: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<RunOutcome, String> {
    let mut child = Command::new(program)
        .args(args)
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(RunOutcome::Exited(status));
        }
        if Instant::now() >= deadline {
            return Ok(RunOutcome::TimedOut);
        }
        thread::sleep(Duration::from_millis(5));
    }
}
