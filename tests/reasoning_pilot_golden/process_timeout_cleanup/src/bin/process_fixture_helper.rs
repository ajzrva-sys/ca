use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("tree") => {
            let pid_file = args.next().expect("missing pid file");
            let child = Command::new(std::env::current_exe().expect("missing executable"))
                .arg("sleep")
                // The hidden verifier captures the tree parent's output. A
                // deliberately surviving descendant must not keep those
                // pipes open, or the verifier cannot observe the timeout.
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to spawn descendant");
            let helper_pid = unsafe { getpid() };
            let process_group = unsafe { getpgrp() };
            std::fs::write(
                pid_file,
                format!(
                    "helper={helper_pid}\ngroup={process_group}\ndescendant={}",
                    child.id()
                ),
            )
            .expect("failed to write pid file");
            thread::sleep(Duration::from_secs(30));
        }
        Some("sleep") => thread::sleep(Duration::from_secs(30)),
        _ => panic!("invalid helper mode"),
    }
}

extern "C" {
    fn getpid() -> i32;
    fn getpgrp() -> i32;
}
