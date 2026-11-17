//! Per-path FIFO lock scheduler.
//!
//! Readers share. Writers are exclusive. A queued writer is not skipped by a
//! later reader. [`LockTable::try_begin`] returns [`Admission::Ready`] or
//! [`Admission::Queued`] and does not block. [`LockTable::end`] drops one
//! holder. [`release`] wakes the FIFO head.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// Shared read or exclusive write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// Immediate grant, or a place at the tail of that path's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Ready,
    #[cfg(test)]
    Queued,
}

#[derive(Debug)]
struct QueuedWait {
    access: Access,
    granted: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct PathState {
    readers: usize,
    writer: bool,
    queue: VecDeque<QueuedWait>,
}

#[derive(Debug, Default)]
struct Inner {
    paths: HashMap<PathBuf, PathState>,
}

/// Scheduler. [`LockTable::try_begin`] does not block. [`LockTable::lock_until_ready`] waits.
#[derive(Debug)]
pub struct LockTable {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl Default for LockTable {
    fn default() -> Self {
        Self::new()
    }
}

impl LockTable {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            cv: Condvar::new(),
        }
    }

    /// Block until `access` holds `path`. A queued writer is not skipped.
    pub fn lock_until_ready(&self, path: impl AsRef<Path>, access: Access) {
        let path = path.as_ref().to_path_buf();
        let mut guard = self.lock();
        let granted = {
            let state = guard.paths.entry(path).or_default();
            if !blocked(state, access) {
                grant(state, access);
                None
            } else {
                Some(enqueue(state, access))
            }
        };
        let Some(granted) = granted else {
            return;
        };
        while !granted.load(Ordering::Acquire) {
            guard = self
                .cv
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Grant `access` on `path`, or queue it behind earlier waiters.
    #[cfg(test)]
    #[must_use]
    pub fn try_begin(&self, path: impl AsRef<Path>, access: Access) -> Admission {
        let path = path.as_ref().to_path_buf();
        let mut inner = self.lock();
        let state = inner.paths.entry(path).or_default();
        let admission = begin(state, access);
        debug_assert!(consistent(state));
        admission
    }

    /// Drop one held `access` on `path`. Does not wake waiters; call [`release`].
    pub fn end(&self, path: impl AsRef<Path>, access: Access) -> Result<(), String> {
        let path = path.as_ref();
        let mut inner = self.lock();
        let Some(state) = inner.paths.get_mut(path) else {
            return Err(missing_holder(path, access));
        };
        match access {
            Access::Read if state.readers == 0 => return Err(missing_holder(path, access)),
            Access::Read => state.readers -= 1,
            Access::Write if !state.writer => return Err(missing_holder(path, access)),
            Access::Write => state.writer = false,
        }
        debug_assert!(consistent(state));
        if idle(state) {
            inner.paths.remove(path);
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wake(&self, path: &Path) -> Vec<Access> {
        let mut inner = self.lock();
        let Some(state) = inner.paths.get_mut(path) else {
            return Vec::new();
        };
        let woken = wake(state);
        debug_assert!(consistent(state));
        if idle(state) {
            inner.paths.remove(path);
        }
        self.cv.notify_all();
        woken
    }
}

/// Wake the FIFO head when `path` is free.
///
/// One writer, or every leading reader, becomes the holder. A queued writer
/// stops the scan, so later readers stay queued. A busy path wakes nobody.
#[must_use]
/// Wait until every access holds its path. A queued writer is not skipped by a later reader.
pub fn admit_batch(paths: &[(&str, Access)]) -> Vec<Admission> {
    let table = process_table();
    paths
        .iter()
        .map(|(path, access)| {
            table.lock_until_ready(Path::new(path), *access);
            Admission::Ready
        })
        .collect()
}

/// Block until `access` holds `path` on the process table.
#[cfg(test)]
pub fn acquire(path: &str, access: Access) {
    process_table().lock_until_ready(path, access);
}

/// How many waiters are behind the current holder of `path`.
#[cfg(test)]
pub fn queued_waiters(path: &str) -> usize {
    let table = process_table();
    let guard = table.lock();
    guard
        .paths
        .get(std::path::Path::new(path))
        .map(|state| state.queue.len())
        .unwrap_or(0)
}

/// Drop one held access and wake the FIFO head. A missing holder is ignored.
pub fn release_held(path: &str, access: Access) {
    let table = process_table();
    if table.end(path, access).is_ok() {
        let _ = release(table, path);
    }
}

fn process_table() -> &'static LockTable {
    static TABLE: OnceLock<LockTable> = OnceLock::new();
    TABLE.get_or_init(LockTable::new)
}

pub fn release(table: &LockTable, path: impl AsRef<Path>) -> Vec<Access> {
    table.wake(path.as_ref())
}

fn blocked(state: &PathState, access: Access) -> bool {
    let writer_needs_quiet = matches!(access, Access::Write) && state.readers > 0;
    state.writer || !state.queue.is_empty() || writer_needs_quiet
}

fn grant(state: &mut PathState, access: Access) {
    match access {
        Access::Read => state.readers += 1,
        Access::Write => state.writer = true,
    }
}

fn enqueue(state: &mut PathState, access: Access) -> Arc<AtomicBool> {
    let granted = Arc::new(AtomicBool::new(false));
    state.queue.push_back(QueuedWait {
        access,
        granted: Arc::clone(&granted),
    });
    granted
}

#[cfg(test)]
fn begin(state: &mut PathState, access: Access) -> Admission {
    if blocked(state, access) {
        let _ = enqueue(state, access);
        return Admission::Queued;
    }
    grant(state, access);
    Admission::Ready
}

fn wake(state: &mut PathState) -> Vec<Access> {
    if state.readers > 0 || state.writer {
        return Vec::new();
    }
    let mut woken = Vec::new();
    loop {
        let next_access = state.queue.front().map(|next| next.access);
        match next_access {
            Some(Access::Write) if !woken.is_empty() => break,
            Some(Access::Write) => {
                let next = state.queue.pop_front().expect("queued writer");
                state.writer = true;
                next.granted.store(true, Ordering::Release);
                woken.push(Access::Write);
                break;
            }
            Some(Access::Read) => {
                let next = state.queue.pop_front().expect("queued reader");
                state.readers += 1;
                next.granted.store(true, Ordering::Release);
                woken.push(Access::Read);
            }
            None => break,
        }
    }
    woken
}

fn idle(state: &PathState) -> bool {
    state.readers == 0 && !state.writer && state.queue.is_empty()
}

fn consistent(state: &PathState) -> bool {
    !(state.writer && state.readers > 0)
}

fn missing_holder(path: &Path, access: Access) -> String {
    let kind = match access {
        Access::Read => "reader",
        Access::Write => "writer",
    };
    format!("no {kind} holds {}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{release, Access, Admission, LockTable};
    use std::path::Path;

    #[test]
    fn two_writes_do_not_interleave() {
        let table = LockTable::new();
        let path = Path::new("/work/file");
        let mut order = Vec::new();

        assert_eq!(table.try_begin(path, Access::Write), Admission::Ready);
        order.push("enter");
        assert_eq!(table.try_begin(path, Access::Write), Admission::Queued);
        table.end(path, Access::Write).expect("first writer holds");
        order.push("leave");
        assert_eq!(release(&table, path), vec![Access::Write]);
        order.push("enter");
        table.end(path, Access::Write).expect("second writer holds");
        order.push("leave");
        assert!(release(&table, path).is_empty());
        assert_eq!(order, ["enter", "leave", "enter", "leave"]);
    }

    #[test]
    fn reader_cannot_enter_while_a_writer_holds() {
        let table = LockTable::new();
        let path = Path::new("/work/file");
        assert_eq!(table.try_begin(path, Access::Write), Admission::Ready);
        assert_eq!(table.try_begin(path, Access::Read), Admission::Queued);
        assert!(release(&table, path).is_empty());
    }

    #[test]
    fn readers_share_until_a_queued_writer_blocks_later_readers() {
        let table = LockTable::new();
        let path = Path::new("/work/file");
        assert_eq!(table.try_begin(path, Access::Read), Admission::Ready);
        assert_eq!(table.try_begin(path, Access::Read), Admission::Ready);
        assert_eq!(table.try_begin(path, Access::Write), Admission::Queued);
        assert_eq!(table.try_begin(path, Access::Read), Admission::Queued);
        assert!(release(&table, path).is_empty());

        table.end(path, Access::Read).expect("reader");
        assert!(release(&table, path).is_empty());
        table.end(path, Access::Read).expect("last reader");
        assert_eq!(release(&table, path), vec![Access::Write]);
        table.end(path, Access::Write).expect("writer");
        assert_eq!(release(&table, path), vec![Access::Read]);
    }

    #[test]
    fn queued_readers_run_before_a_writer_queued_ahead_of_later_readers() {
        let table = LockTable::new();
        let path = Path::new("/work/file");
        let mut order = Vec::new();

        assert_eq!(table.try_begin(path, Access::Write), Admission::Ready);
        order.push("enter-w0");
        assert_eq!(table.try_begin(path, Access::Read), Admission::Queued);
        assert_eq!(table.try_begin(path, Access::Read), Admission::Queued);
        assert_eq!(table.try_begin(path, Access::Write), Admission::Queued);
        assert_eq!(table.try_begin(path, Access::Read), Admission::Queued);

        table.end(path, Access::Write).expect("writer finished");
        order.push("leave-w0");
        assert_eq!(release(&table, path), vec![Access::Read, Access::Read]);
        order.push("enter-r1");
        order.push("enter-r2");

        table.end(path, Access::Read).expect("r1");
        assert!(release(&table, path).is_empty());
        table.end(path, Access::Read).expect("r2");
        order.push("leave-r1");
        order.push("leave-r2");
        assert_eq!(release(&table, path), vec![Access::Write]);
        order.push("enter-w1");
        table.end(path, Access::Write).expect("w1");
        order.push("leave-w1");
        assert_eq!(release(&table, path), vec![Access::Read]);
        order.push("enter-r3");

        assert_eq!(
            order,
            [
                "enter-w0", "leave-w0", "enter-r1", "enter-r2", "leave-r1", "leave-r2", "enter-w1",
                "leave-w1", "enter-r3",
            ]
        );
    }

    #[test]
    fn distinct_paths_do_not_share_a_queue() {
        let table = LockTable::new();
        assert_eq!(table.try_begin("/work/a", Access::Write), Admission::Ready);
        assert_eq!(table.try_begin("/work/b", Access::Write), Admission::Ready);
    }
}
