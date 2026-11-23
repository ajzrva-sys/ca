//! A whole-file overwrite needs a full read in this turn, and the bytes must
//! still match. A ranged edit does not come through here.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct Baselines {
    turn: u64,
    files: HashMap<PathBuf, Baseline>,
}

#[derive(Clone, Debug)]
struct Baseline {
    turn: u64,
    sha256: String,
    through_line: usize,
    total_lines: usize,
    full: bool,
}

impl Baselines {
    pub fn new() -> Self {
        Self {
            turn: 1,
            files: HashMap::new(),
        }
    }

    pub fn begin_turn(&mut self) {
        self.turn = self.turn.saturating_add(1);
        self.files.clear();
    }

    pub fn note_read(
        &mut self,
        path: &Path,
        start: usize,
        end: usize,
        total_lines: usize,
        bytes: &[u8],
    ) {
        self.note_digest(path, start, end, total_lines, &hash(bytes));
    }

    pub fn note_digest(
        &mut self,
        path: &Path,
        start: usize,
        end: usize,
        total_lines: usize,
        sha256: &str,
    ) {
        if start == 0 {
            return;
        }
        let sha256 = sha256.to_string();
        let entry = self
            .files
            .entry(path.to_path_buf())
            .or_insert_with(|| Baseline {
                turn: self.turn,
                sha256: sha256.clone(),
                through_line: 0,
                total_lines,
                full: false,
            });
        if entry.turn != self.turn || entry.sha256 != sha256 {
            entry.turn = self.turn;
            entry.sha256 = sha256;
            entry.through_line = 0;
            entry.full = false;
        }
        entry.total_lines = total_lines;
        let reached = end.min(total_lines);
        if start == 1 || start == entry.through_line.saturating_add(1) {
            entry.through_line = entry.through_line.max(reached);
        }
        entry.full = total_lines == 0 || entry.through_line >= total_lines;
    }

    pub fn remember_written(&mut self, path: &Path, bytes: &[u8]) {
        let total_lines = bytes.split(|byte| *byte == b'\n').count();
        self.files.insert(
            path.to_path_buf(),
            Baseline {
                turn: self.turn,
                sha256: hash(bytes),
                through_line: total_lines,
                total_lines,
                full: true,
            },
        );
    }

    pub fn read_in_full(&self, path: &Path, current: &[u8]) -> bool {
        let sha256 = hash(current);
        self.files
            .get(path)
            .is_some_and(|entry| entry.turn == self.turn && entry.full && entry.sha256 == sha256)
    }

    pub fn overwrite_refusal(&self, path: &Path, current: &[u8]) -> Option<String> {
        let sha256 = hash(current);
        let fresh = self
            .files
            .get(path)
            .is_some_and(|entry| entry.turn == self.turn && entry.full && entry.sha256 == sha256);
        if fresh {
            return None;
        }
        Some(format!(
            "{path} was modified since you last read it, or this turn has not read the whole file. Re-read the file before writing, or use a ranged edit for a partial change. The file was not modified.",
            path = path.display()
        ))
    }
}

fn registry() -> &'static Mutex<Baselines> {
    static REGISTRY: OnceLock<Mutex<Baselines>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Baselines::new()))
}

fn lock() -> std::sync::MutexGuard<'static, Baselines> {
    registry()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

pub fn begin_turn() {
    lock().begin_turn();
}

pub fn note_read(path: &Path, start: usize, end: usize, total_lines: usize, bytes: &[u8]) {
    lock().note_read(path, start, end, total_lines, bytes);
}

pub fn note_digest(path: &Path, start: usize, end: usize, total_lines: usize, sha256: &str) {
    lock().note_digest(path, start, end, total_lines, sha256);
}

pub fn remember_written(path: &Path, bytes: &[u8]) {
    lock().remember_written(path, bytes);
}

pub fn read_in_full(path: &Path, current: &[u8]) -> bool {
    lock().read_in_full(path, current)
}

pub fn overwrite_refusal(path: &Path, current: &[u8]) -> Option<String> {
    lock().overwrite_refusal(path, current)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_file_refuses_an_overwrite_without_a_fresh_full_read() {
        let mut baselines = Baselines::new();
        let path = Path::new("/tmp/ca-write-baseline-example.txt");
        let original = b"one\ntwo\nthree\n";
        assert!(baselines.overwrite_refusal(path, original).is_some());
        baselines.note_read(path, 1, 1, 3, original);
        assert!(baselines.overwrite_refusal(path, original).is_some());
        baselines.note_read(path, 2, 3, 3, original);
        assert!(baselines.overwrite_refusal(path, original).is_none());
        assert!(baselines.overwrite_refusal(path, b"changed\n").is_some());
        baselines.note_read(path, 1, 3, 3, original);
        baselines.begin_turn();
        assert!(baselines.overwrite_refusal(path, original).is_some());
        baselines.note_read(path, 1, 3, 3, original);
        baselines.remember_written(path, b"replacement\n");
        assert!(baselines
            .overwrite_refusal(path, b"replacement\n")
            .is_none());
    }
}
