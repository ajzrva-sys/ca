use std::path::Path;
use std::time::Duration;

use reasoning_pilot_process_timeout_cleanup::{run_with_timeout, RunOutcome};

#[test]
#[cfg(unix)]
fn successful_process_returns_status() {
    let result = run_with_timeout(Path::new("/usr/bin/true"), &[], Duration::from_secs(1)).unwrap();
    assert!(matches!(result, RunOutcome::Exited(status) if status.success()));
}
