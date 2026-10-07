use std::collections::HashSet;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

const IDLE_RELEASE_DEBOUNCE: Duration = Duration::from_millis(250);
const HELPER_STOP_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAwakeAvailability {
    Disabled,
    Supported,
    Unsupported(&'static str),
}

impl KeepAwakeAvailability {
    pub fn warning(self) -> Option<String> {
        match self {
            Self::Unsupported(reason) => Some(format!(
                "Keep-awake is unavailable ({reason}); continuing without sleep inhibition"
            )),
            Self::Disabled | Self::Supported => None,
        }
    }
}

#[derive(Debug, Clone)]
struct HelperSpec {
    program: &'static str,
    arguments: Vec<String>,
}

#[derive(Debug)]
enum ManagerCommand {
    Acquire(u64),
    Release(u64),
    StopIfIdle(u64),
    Shutdown(oneshot::Sender<()>),
}

#[derive(Debug, Clone)]
struct KeepAwakeManager {
    sender: mpsc::UnboundedSender<ManagerCommand>,
    next_id: Arc<AtomicU64>,
}

impl KeepAwakeManager {
    fn start(spec: HelperSpec) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let manager = Self {
            sender: sender.clone(),
            next_id: Arc::new(AtomicU64::new(1)),
        };
        tokio::spawn(run_manager(receiver, sender, spec));
        manager
    }

    fn acquire(&self) -> Option<KeepAwakeLease> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.sender.send(ManagerCommand::Acquire(id)).ok()?;
        Some(KeepAwakeLease {
            id,
            sender: self.sender.clone(),
            released: false,
        })
    }

    async fn shutdown(&self) {
        let (sender, receiver) = oneshot::channel();
        if self.sender.send(ManagerCommand::Shutdown(sender)).is_ok() {
            let _ = tokio::time::timeout(Duration::from_secs(2), receiver).await;
        }
    }
}

pub struct KeepAwakeLease {
    id: u64,
    sender: mpsc::UnboundedSender<ManagerCommand>,
    released: bool,
}

impl std::fmt::Debug for KeepAwakeLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeepAwakeLease")
            .field("active", &!self.released)
            .finish()
    }
}

impl KeepAwakeLease {
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if !self.released {
            self.released = true;
            let _ = self.sender.send(ManagerCommand::Release(self.id));
        }
    }
}

impl Drop for KeepAwakeLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[derive(Debug)]
struct ProcessKeepAwake {
    availability: KeepAwakeAvailability,
    manager: Option<KeepAwakeManager>,
}

static PROCESS_KEEP_AWAKE: OnceLock<ProcessKeepAwake> = OnceLock::new();

pub fn configure_process(enabled: bool) -> KeepAwakeAvailability {
    let configured = PROCESS_KEEP_AWAKE.get_or_init(|| {
        if !enabled {
            return ProcessKeepAwake {
                availability: KeepAwakeAvailability::Disabled,
                manager: None,
            };
        }
        match platform_helper() {
            Ok(spec) => ProcessKeepAwake {
                availability: KeepAwakeAvailability::Supported,
                manager: Some(KeepAwakeManager::start(spec)),
            },
            Err(reason) => ProcessKeepAwake {
                availability: KeepAwakeAvailability::Unsupported(reason),
                manager: None,
            },
        }
    });
    configured.availability
}

pub fn acquire_process() -> Option<KeepAwakeLease> {
    PROCESS_KEEP_AWAKE
        .get()
        .and_then(|configured| configured.manager.as_ref())
        .and_then(KeepAwakeManager::acquire)
}

pub async fn shutdown_process() {
    if let Some(manager) = PROCESS_KEEP_AWAKE
        .get()
        .and_then(|configured| configured.manager.as_ref())
    {
        manager.shutdown().await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackerEffect {
    None,
    Start,
    ScheduleStop(u64),
    Stop,
}

#[derive(Debug, Default)]
struct LeaseTracker {
    leases: HashSet<u64>,
    helper_running: bool,
    generation: u64,
}

impl LeaseTracker {
    fn acquire(&mut self, id: u64) -> TrackerEffect {
        self.generation = self.generation.wrapping_add(1);
        if !self.leases.insert(id) || self.helper_running {
            return TrackerEffect::None;
        }
        self.helper_running = true;
        TrackerEffect::Start
    }

    fn release(&mut self, id: u64) -> TrackerEffect {
        if !self.leases.remove(&id) || !self.leases.is_empty() || !self.helper_running {
            return TrackerEffect::None;
        }
        self.generation = self.generation.wrapping_add(1);
        TrackerEffect::ScheduleStop(self.generation)
    }

    fn stop_if_idle(&mut self, generation: u64) -> TrackerEffect {
        if generation != self.generation || !self.leases.is_empty() || !self.helper_running {
            return TrackerEffect::None;
        }
        self.helper_running = false;
        TrackerEffect::Stop
    }

    fn start_failed(&mut self) {
        self.helper_running = false;
    }

    fn shutdown(&mut self) -> TrackerEffect {
        self.leases.clear();
        self.generation = self.generation.wrapping_add(1);
        if std::mem::take(&mut self.helper_running) {
            TrackerEffect::Stop
        } else {
            TrackerEffect::None
        }
    }
}

async fn run_manager(
    mut receiver: mpsc::UnboundedReceiver<ManagerCommand>,
    sender: mpsc::UnboundedSender<ManagerCommand>,
    spec: HelperSpec,
) {
    let mut tracker = LeaseTracker::default();
    let mut helper = None;
    while let Some(command) = receiver.recv().await {
        let effect = match command {
            ManagerCommand::Acquire(id) => tracker.acquire(id),
            ManagerCommand::Release(id) => tracker.release(id),
            ManagerCommand::StopIfIdle(generation) => tracker.stop_if_idle(generation),
            ManagerCommand::Shutdown(acknowledgement) => {
                if tracker.shutdown() == TrackerEffect::Stop {
                    stop_helper(&mut helper).await;
                }
                let _ = acknowledgement.send(());
                return;
            }
        };
        match effect {
            TrackerEffect::Start => {
                helper = start_helper(&spec);
                if helper.is_none() {
                    tracker.start_failed();
                }
            }
            TrackerEffect::ScheduleStop(generation) => {
                let sender = sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(IDLE_RELEASE_DEBOUNCE).await;
                    let _ = sender.send(ManagerCommand::StopIfIdle(generation));
                });
            }
            TrackerEffect::Stop => stop_helper(&mut helper).await,
            TrackerEffect::None => {}
        }
    }
    stop_helper(&mut helper).await;
}

fn start_helper(spec: &HelperSpec) -> Option<Child> {
    let mut command = Command::new(spec.program);
    command
        .args(&spec.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() == 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "parent exited before keep-awake helper spawn",
                ));
            }
            Ok(())
        });
    }
    command.spawn().ok()
}

async fn stop_helper(helper: &mut Option<Child>) {
    let Some(mut child) = helper.take() else {
        return;
    };
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(unix)]
    if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    if tokio::time::timeout(HELPER_STOP_GRACE, child.wait())
        .await
        .is_err()
    {
        #[cfg(unix)]
        if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

#[cfg(target_os = "macos")]
fn platform_helper() -> Result<HelperSpec, &'static str> {
    const PROGRAM: &str = "/usr/bin/caffeinate";
    if !std::fs::metadata(PROGRAM).is_ok_and(|metadata| metadata.is_file()) {
        return Err("/usr/bin/caffeinate is unavailable");
    }
    Ok(HelperSpec {
        program: PROGRAM,
        arguments: vec!["-i".into(), "-w".into(), std::process::id().to_string()],
    })
}

#[cfg(target_os = "linux")]
fn platform_helper() -> Result<HelperSpec, &'static str> {
    const PROGRAM: &str = "/usr/bin/systemd-inhibit";
    const SLEEP: &str = "/bin/sleep";
    if !std::fs::metadata(PROGRAM).is_ok_and(|metadata| metadata.is_file()) {
        return Err("/usr/bin/systemd-inhibit is unavailable");
    }
    if !std::fs::metadata(SLEEP).is_ok_and(|metadata| metadata.is_file()) {
        return Err("/bin/sleep is unavailable");
    }
    Ok(HelperSpec {
        program: PROGRAM,
        arguments: vec![
            "--what=idle".into(),
            "--mode=block".into(),
            "--who=ca".into(),
            "--why=active model work".into(),
            SLEEP.into(),
            "infinity".into(),
        ],
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_helper() -> Result<HelperSpec, &'static str> {
    Err("this operating system is unsupported")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeHelper {
        starts: usize,
        stops: usize,
    }

    impl FakeHelper {
        fn apply(&mut self, effect: TrackerEffect) {
            match effect {
                TrackerEffect::Start => self.starts += 1,
                TrackerEffect::Stop => self.stops += 1,
                TrackerEffect::None | TrackerEffect::ScheduleStop(_) => {}
            }
        }
    }

    #[test]
    fn fake_helper_is_reference_counted_and_idle_release_is_debounced() {
        let mut tracker = LeaseTracker::default();
        let mut helper = FakeHelper::default();
        helper.apply(tracker.acquire(1));
        helper.apply(tracker.acquire(2));
        assert_eq!((helper.starts, helper.stops), (1, 0));
        assert_eq!(tracker.release(1), TrackerEffect::None);
        let TrackerEffect::ScheduleStop(first_generation) = tracker.release(2) else {
            panic!("last release must schedule helper cleanup");
        };
        helper.apply(tracker.acquire(3));
        helper.apply(tracker.stop_if_idle(first_generation));
        assert_eq!((helper.starts, helper.stops), (1, 0));
        let TrackerEffect::ScheduleStop(second_generation) = tracker.release(3) else {
            panic!("new last release must schedule cleanup");
        };
        helper.apply(tracker.stop_if_idle(second_generation));
        assert_eq!((helper.starts, helper.stops), (1, 1));
    }

    #[test]
    fn fake_helper_cleanup_is_idempotent_across_duplicate_release_and_shutdown() {
        let mut tracker = LeaseTracker::default();
        let mut helper = FakeHelper::default();
        helper.apply(tracker.acquire(7));
        assert_eq!(tracker.release(99), TrackerEffect::None);
        helper.apply(tracker.shutdown());
        helper.apply(tracker.shutdown());
        assert_eq!((helper.starts, helper.stops), (1, 1));
        assert!(tracker.leases.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_helper_is_fixed_absolute_and_idle_only() {
        let helper = platform_helper().unwrap();
        assert_eq!(helper.program, "/usr/bin/caffeinate");
        assert_eq!(
            helper.arguments,
            ["-i", "-w", &std::process::id().to_string()]
        );
        assert!(!helper.arguments.iter().any(|argument| argument == "-d"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_helper_is_fixed_absolute_idle_only_and_wraps_fixed_sleep() {
        let Ok(helper) = platform_helper() else {
            return;
        };
        assert_eq!(helper.program, "/usr/bin/systemd-inhibit");
        assert_eq!(
            helper.arguments,
            [
                "--what=idle",
                "--mode=block",
                "--who=ca",
                "--why=active model work",
                "/bin/sleep",
                "infinity",
            ]
        );
    }
}
