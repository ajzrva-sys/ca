//! Path refusal for device and proc files that can hang a read or leak process
//! state. The check is lexical. It does not open, stat, or canonicalize.

use std::path::{Component, Path};

pub const REFUSAL: &str = "Refused: this path can hang a read or leak process state.";

const BLOCKED_DEVICES: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/full",
    "/dev/stdin",
    "/dev/tty",
    "/dev/console",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/fd/0",
    "/dev/fd/1",
    "/dev/fd/2",
];

const BLOCKED_PROC_SUFFIXES: &[&str] = &[
    "/fd/0",
    "/fd/1",
    "/fd/2",
    "/environ",
    "/cmdline",
    "/maps",
    "/smaps",
    "/smaps_rollup",
    "/numa_maps",
    "/mem",
    "/auxv",
    "/pagemap",
];

pub fn refusal(path: &Path) -> Option<&'static str> {
    let normalized = normalize(path);
    if BLOCKED_DEVICES.iter().any(|device| normalized == *device) {
        return Some(REFUSAL);
    }
    if normalized.starts_with("/proc/")
        && BLOCKED_PROC_SUFFIXES
            .iter()
            .any(|suffix| normalized.ends_with(suffix))
    {
        return Some(REFUSAL);
    }
    None
}

fn normalize(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir => parts.clear(),
            Component::Normal(part) => parts.push(part.to_string_lossy().replace('\\', "/")),
            Component::Prefix(_) => return String::new(),
        }
    }
    if path.is_absolute() {
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn device_and_proc_paths_are_refused_before_open() {
        for path in [
            "/dev/zero",
            "/dev/random",
            "/dev/urandom",
            "/dev/full",
            "/dev/tty",
            "/dev/stdin",
            "/dev/fd/0",
            "/dev/./zero",
            "/proc/42/environ",
            "/proc/self/cmdline",
            "/proc/1/task/9/maps",
            "/proc/1/smaps",
            "/proc/1/smaps_rollup",
            "/proc/1/numa_maps",
            "/proc/1/mem",
            "/proc/1/auxv",
            "/proc/1/pagemap",
            "/proc/1/fd/1",
        ] {
            assert_eq!(refusal(Path::new(path)), Some(REFUSAL), "{path}");
        }
        assert_eq!(refusal(Path::new("/proc/cpuinfo")), None);
        assert_eq!(refusal(Path::new("/proc/1/status")), None);
        assert_eq!(refusal(Path::new("/tmp/notes.txt")), None);
        assert_eq!(refusal(Path::new("dev/zero")), None);
    }
}
