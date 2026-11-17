//! One restricted-flow rule. A restricted read and an outbound admission share
//! one lock, so a concurrent read cannot send unrestricted.
#![allow(dead_code)]
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowLabel {
    Unrestricted,
    Restricted,
}

#[derive(Debug)]
struct GateState {
    label: FlowLabel,
}

#[derive(Debug)]
pub struct FlowGate {
    restricted_paths: BTreeSet<String>,
    approved_destinations: BTreeSet<String>,
    state: Mutex<GateState>,
    /// Senders that have entered admission and not yet taken `state`.
    waiters: std::sync::atomic::AtomicUsize,
}

/// Holds the restricted-read critical section until dropped.
pub struct ReadHold<'a> {
    guard: std::sync::MutexGuard<'a, GateState>,
}

impl FlowGate {
    pub fn new(
        paths: impl IntoIterator<Item = impl Into<String>>,
        destinations: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            restricted_paths: paths.into_iter().map(Into::into).collect(),
            approved_destinations: destinations.into_iter().map(Into::into).collect(),
            state: Mutex::new(GateState {
                label: FlowLabel::Unrestricted,
            }),
            waiters: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn restricted_path(&self, path: &Path) -> bool {
        self.restricted_paths
            .iter()
            .any(|root| path.starts_with(root))
    }

    fn decide(
        &self,
        label: FlowLabel,
        destination: &str,
        destination_lineage_restricted: bool,
    ) -> Result<FlowLabel, &'static str> {
        if label == FlowLabel::Restricted && !self.approved_destinations.contains(destination) {
            return Err("restricted data cannot use an unapproved destination");
        }
        if label == FlowLabel::Restricted && !destination_lineage_restricted {
            return Err("restricted context cannot enter an unrestricted lineage");
        }
        Ok(label)
    }

    /// Label the read and keep the admission lock until the hold is dropped.
    pub fn begin_read(&self, path: &Path) -> ReadHold<'_> {
        let mut guard = self.lock();
        if self.restricted_path(path) {
            guard.label = FlowLabel::Restricted;
        }
        ReadHold { guard }
    }

    pub fn label_read(&self, path: &Path) -> FlowLabel {
        self.begin_read(path).guard.label
    }

    pub fn admit_send(
        &self,
        destination: &str,
        destination_lineage_restricted: bool,
    ) -> Result<FlowLabel, &'static str> {
        self.waiters
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let guard = self.lock();
        self.waiters
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        self.decide(guard.label, destination, destination_lineage_restricted)
    }

    pub fn waiters(&self) -> usize {
        self.waiters.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn label_missing_provenance(&self) -> FlowLabel {
        let mut guard = self.lock();
        guard.label = FlowLabel::Restricted;
        guard.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn restricted_read_blocks_unapproved_send() {
        let gate = Arc::new(FlowGate::new(["/secret"], ["approved-dest"]));
        let read = gate.label_read(Path::new("/secret/file"));
        assert_eq!(read, FlowLabel::Restricted);
        assert!(gate.admit_send("other-dest", true).is_err());
        assert!(gate.admit_send("approved-dest", false).is_err());
        assert_eq!(
            gate.admit_send("approved-dest", true).unwrap(),
            FlowLabel::Restricted
        );
        assert_eq!(
            FlowGate::new(["/secret"], ["approved-dest"]).label_missing_provenance(),
            FlowLabel::Restricted
        );
    }

    #[test]
    fn restricted_label_change_cannot_send_unrestricted() {
        let gate = Arc::new(FlowGate::new(["/secret"], ["approved-dest"]));
        let hold = gate.begin_read(Path::new("/secret/file"));
        assert_eq!(hold.guard.label, FlowLabel::Restricted);
        let sender = Arc::clone(&gate);
        let raced = std::thread::spawn(move || sender.admit_send("other-dest", false));
        while gate.waiters() == 0 {
            std::thread::yield_now();
        }
        drop(hold);
        let raced = raced.join().unwrap();
        assert_eq!(
            raced,
            Err("restricted data cannot use an unapproved destination")
        );
        assert_eq!(
            gate.admit_send("approved-dest", false),
            Err("restricted context cannot enter an unrestricted lineage")
        );
    }
}
