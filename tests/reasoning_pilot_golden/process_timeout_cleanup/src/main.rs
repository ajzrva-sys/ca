use std::path::Path;
use std::time::Duration;

use reasoning_pilot_process_timeout_cleanup::{run_tree_with_timeout, RunOutcome};

fn main() {
    let mut args = std::env::args().skip(1);
    let helper = args.next().expect("missing helper path");
    let pid_file = args.next().expect("missing pid file");
    let timeout_ms = args
        .next()
        .expect("missing timeout")
        .parse::<u64>()
        .expect("invalid timeout");
    assert!(args.next().is_none(), "unexpected argument");
    let helper_args = vec!["tree".to_string(), pid_file.clone()];
    match run_tree_with_timeout(
        Path::new(&helper),
        &helper_args,
        Duration::from_millis(timeout_ms),
        Path::new(&pid_file),
    )
    .expect("runner failed")
    {
        RunOutcome::Exited(status) => println!("exited:{:?}", status.code()),
        RunOutcome::TimedOut => println!("timed_out"),
    }
}
