#!/usr/bin/env python3
"""Remove regenerable build output and scratch task trees.

Deletes Cargo target directories, private Cargo homes, node_modules,
Python caches, Finder .DS_Store files, and scratch/. scratch/ is where
scripts/verify.py and scripts/check-change.py write task copies, logs,
and receipts; the next run creates it again. oracles/ and reference/
are not touched. A Cargo target or the check-change checkout is left
in place while its lock is held.
"""

from __future__ import annotations

import argparse
import fcntl
import os
from pathlib import Path
import subprocess
import sys
import tempfile


PRESERVE_TREES = {"oracles", "reference"}
CACHE_DIRS = {"__pycache__", ".pytest_cache", ".mypy_cache"}
CARGO_HOMES = {".cargo", "cargo-home"}
CREDENTIAL_NAMES = ("credentials", "credentials.toml")


class HeldLock:
    def __init__(self, fd):
        self.fd = fd

    def close(self):
        if self.fd >= 0:
            os.close(self.fd)
            self.fd = -1


class Item:
    def __init__(self, path, reason, locks):
        self.path = path
        self.reason = reason
        self.locks = locks


def repository_root():
    return Path(__file__).resolve().parents[1]


def display(root, path):
    try:
        shown = path.relative_to(root)
    except ValueError:
        shown = path
    return "." if shown == Path(".") else str(shown)


def is_cargo_target(path):
    """True for a Cargo target dir, including the live profile layout.

    Cargo writes CACHEDIR.TAG at the target root when it can, but this tree
    often has only target/.rustc_info.json and target/<profile>/.fingerprint.
    A directory named target without those markers (docs/target) is not one.
    """
    if not path.is_dir() or path.is_symlink():
        return False
    if (path / "CACHEDIR.TAG").is_file() or (path / ".fingerprint").is_dir():
        return True
    if (path / ".rustc_info.json").is_file():
        return True
    try:
        for child in path.iterdir():
            if child.is_dir() and not child.is_symlink() and (child / ".fingerprint").is_dir():
                return True
    except OSError:
        return False
    return False


def has_credentials(path):
    return any((path / name).exists() for name in CREDENTIAL_NAMES)


def try_lock(path):
    """Hold an existing flock, report 'busy', or return None when absent."""
    if not path.is_file():
        return None
    fd = os.open(path, os.O_RDWR)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(fd)
        return "busy"
    return HeldLock(fd)


def close_locks(locks):
    for lock in locks:
        lock.close()


def collect_locks(directory):
    """Lock check-change and Cargo targets under directory without descending into them.

    Returns (held locks, busy reason). The caller closes held locks.
    """
    held = []
    if not directory.is_dir() or directory.is_symlink():
        return held, None
    for dirpath, dirnames, _filenames in os.walk(directory, followlinks=False):
        current = Path(dirpath)
        if current.name in CARGO_HOMES and has_credentials(current):
            return held, "contains Cargo credentials"
        if current.name == "check-change":
            result = try_lock(current / "run.lock")
            if result == "busy":
                return held, "check-change lock held"
            if isinstance(result, HeldLock):
                held.append(result)
        if "target" in dirnames:
            target = current / "target"
            if not target.is_symlink() and is_cargo_target(target):
                result = try_lock(target / ".cargo-lock")
                if result == "busy":
                    return held, "cargo lock held"
                if isinstance(result, HeldLock):
                    held.append(result)
            dirnames.remove("target")
        for name in list(dirnames):
            path = current / name
            if path.is_symlink():
                dirnames.remove(name)
                continue
            if name in CARGO_HOMES and has_credentials(path):
                return held, "contains Cargo credentials"
            if name in CACHE_DIRS or name in CARGO_HOMES or name in {"node_modules", ".git"}:
                dirnames.remove(name)
    return held, None


def cache_item(path):
    """Return an Item when path is a regenerable cache directory, else None or 'busy'."""
    if path.is_symlink() or not path.is_dir():
        return None
    name = path.name
    parent = path.parent
    locks = []
    if name == "target":
        if not is_cargo_target(path):
            return None
        result = try_lock(path / ".cargo-lock")
        if result == "busy":
            return "busy"
        if isinstance(result, HeldLock):
            locks.append(result)
        return Item(path, "cargo target", locks)
    if name == "node_modules" and (parent / "package.json").is_file():
        return Item(path, "node_modules", locks)
    if name in CARGO_HOMES:
        if has_credentials(path):
            return "credentials"
        return Item(path, name, locks)
    if name in CACHE_DIRS:
        return Item(path, name, locks)
    return None


def plan(root, keep_scratch, keep_target):
    """Choose real directories and files to remove. Does not follow symlinks."""
    remove = []
    skip = []
    scratch = root / "scratch"
    check_change_busy = False
    if scratch.is_symlink():
        skip.append((scratch, "symlink"))
    elif scratch.is_dir() and not keep_scratch:
        free = []
        blocked = False
        for child in sorted(scratch.iterdir(), key=lambda item: item.name):
            if child.is_symlink():
                free.append((child, []))
                continue
            held, busy = collect_locks(child)
            if busy:
                close_locks(held)
                skip.append((child, busy))
                blocked = True
                if busy == "check-change lock held":
                    check_change_busy = True
                continue
            free.append((child, held))
        if blocked:
            for child, locks in free:
                remove.append(Item(child, "scratch", locks))
        else:
            locks = [lock for _child, held in free for lock in held]
            remove.append(Item(scratch, "scratch", locks))

    for dirpath, dirnames, filenames in os.walk(root, topdown=True, followlinks=False):
        current = Path(dirpath)
        parts = current.relative_to(root).parts
        if parts[:1] and parts[0] in PRESERVE_TREES:
            dirnames[:] = []
            continue
        if parts[:1] == ("scratch",) and not keep_scratch:
            dirnames[:] = []
            continue
        for name in filenames:
            path = current / name
            if name == ".DS_Store" or name.endswith((".pyc", ".pyo")):
                if not path.is_symlink():
                    remove.append(Item(path, "cache file", []))
        kept = []
        for name in dirnames:
            path = current / name
            if path.is_symlink():
                continue
            if parts == () and name in PRESERVE_TREES:
                kept.append(name)
                continue
            if parts == () and name == "scratch" and not keep_scratch:
                continue
            if parts == () and name == "target" and (keep_target or check_change_busy):
                reason = "kept by --keep-target" if keep_target else "focused check is using it"
                skip.append((path, reason))
                continue
            chosen = cache_item(path)
            if chosen == "busy":
                skip.append((path, "cargo lock held"))
            elif chosen == "credentials":
                skip.append((path, "contains Cargo credentials"))
            elif isinstance(chosen, Item):
                remove.append(chosen)
            else:
                kept.append(name)
        dirnames[:] = kept
    return remove, skip


def remove_path(path):
    if path.is_symlink() or not path.is_dir():
        path.unlink()
        return
    subprocess.run(["rm", "-rf", "--", str(path)], check=True)


def execute(items):
    failed = []
    for item in sorted(items, key=lambda item: len(item.path.parts), reverse=True):
        try:
            if item.path.exists() or item.path.is_symlink():
                remove_path(item.path)
        except (OSError, subprocess.CalledProcessError) as error:
            failed.append((item.path, str(error)))
        finally:
            close_locks(item.locks)
    return failed


def report(root, remove, skip, dry_run):
    verb = "would remove" if dry_run else "removed"
    for item in remove:
        print(f"{verb} {display(root, item.path)}")
    for path, reason in skip:
        print(f"skipped {display(root, path)} ({reason})")
    print(f"{len(remove)} {verb}, {len(skip)} skipped")


def self_test():
    with tempfile.TemporaryDirectory(prefix="ca-clean-") as temporary:
        root = Path(temporary)
        target = root / "target"
        target.mkdir()
        (target / "CACHEDIR.TAG").write_text("cargo cache\n")
        (target / ".fingerprint").mkdir()
        (target / "keep-if-linked").write_text("build")
        (root / ".DS_Store").write_bytes(b"finder")
        package = root / ".mimocode"
        package.mkdir()
        (package / "package.json").write_text("{}\n")
        (package / "node_modules" / "left").mkdir(parents=True)
        (root / "scripts" / "__pycache__").mkdir(parents=True)
        (root / "scripts" / "__pycache__" / "clean.pyc").write_bytes(b"pyc")
        (root / "src").mkdir()
        (root / "src" / "main.rs").write_text("fn main() {}\n")
        (root / "docs" / "target").mkdir(parents=True)
        (root / "docs" / "target" / "notes.md").write_text("keep\n")
        oracle = root / "oracles" / "sample" / "target"
        oracle.mkdir(parents=True)
        (oracle / "CACHEDIR.TAG").write_text("oracle\n")
        reference = root / "reference"
        reference.mkdir()
        (reference / ".DS_Store").write_bytes(b"keep")
        slot = root / "scratch" / "slot"
        slot.mkdir(parents=True)
        (slot / "note.json").write_text("{}\n")
        (slot / "target").symlink_to(target, target_is_directory=True)

        preview, _skip = plan(root, keep_scratch=False, keep_target=True)
        execute(preview)
        assert (target / "keep-if-linked").read_text() == "build"
        assert not slot.exists()
        assert not (root / "scratch").exists()

    with tempfile.TemporaryDirectory(prefix="ca-clean-") as temporary:
        root = Path(temporary)
        target = root / "target"
        target.mkdir()
        (target / "CACHEDIR.TAG").write_text("cargo cache\n")
        (target / ".fingerprint").mkdir()
        (target / ".cargo-lock").write_text("")
        package = root / ".mimocode"
        package.mkdir()
        (package / "package.json").write_text("{}\n")
        (package / "node_modules").mkdir()
        (root / "src").mkdir()
        (root / "src" / "main.rs").write_text("fn main() {}\n")
        (root / "docs" / "target").mkdir(parents=True)
        (root / "docs" / "target" / "notes.md").write_text("keep\n")
        oracle = root / "oracles" / "sample" / "target"
        oracle.mkdir(parents=True)
        (oracle / "CACHEDIR.TAG").write_text("oracle\n")
        (root / "reference" / ".DS_Store").parent.mkdir(parents=True)
        (root / "reference" / ".DS_Store").write_bytes(b"keep")
        (root / ".DS_Store").write_bytes(b"finder")
        (root / "scripts" / "__pycache__").mkdir(parents=True)
        scratch = root / "scratch"
        (scratch / "old.log").parent.mkdir(parents=True)
        (scratch / "old.log").write_text("log\n")
        home = scratch / "run" / ".cargo"
        home.mkdir(parents=True)
        (home / "registry").mkdir()
        credentialed = root / "cargo-home"
        credentialed.mkdir()
        (credentialed / "credentials.toml").write_text("secret\n")
        check = scratch / "check-change"
        check.mkdir()
        (check / "run.lock").write_text("")
        (check / "source").mkdir()
        (check / "source" / "lib.rs").write_text("keep while locked\n")

        cargo_remove = []
        cargo = os.open(target / ".cargo-lock", os.O_RDWR)
        try:
            fcntl.flock(cargo, fcntl.LOCK_EX | fcntl.LOCK_NB)
            cargo_remove, cargo_skip = plan(root, keep_scratch=False, keep_target=False)
            assert target not in {item.path for item in cargo_remove}
            assert any(path == target and reason == "cargo lock held" for path, reason in cargo_skip)
        finally:
            for item in cargo_remove:
                close_locks(item.locks)
            os.close(cargo)

        fd = os.open(check / "run.lock", os.O_RDWR)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            locked_remove, locked_skip = plan(root, keep_scratch=False, keep_target=False)
            assert target not in {item.path for item in locked_remove}
            assert check not in {item.path for item in locked_remove}
            assert scratch / "old.log" in {item.path for item in locked_remove}
            assert any(reason == "check-change lock held" for _path, reason in locked_skip)
            assert any(path == target and reason == "focused check is using it" for path, reason in locked_skip)
            assert not execute(locked_remove)
        finally:
            os.close(fd)
        assert (target / "CACHEDIR.TAG").is_file()
        assert (check / "source" / "lib.rs").is_file()
        assert not (scratch / "old.log").exists()
        assert (credentialed / "credentials.toml").is_file()

        remove, skip = plan(root, keep_scratch=False, keep_target=False)
        assert not execute(remove)
        assert not target.exists()
        assert not scratch.exists()
        assert not (package / "node_modules").exists()
        assert (package / "package.json").is_file()
        assert (root / "src" / "main.rs").is_file()
        assert (root / "docs" / "target" / "notes.md").is_file()
        assert (oracle / "CACHEDIR.TAG").is_file()
        assert (root / "reference" / ".DS_Store").is_file()
        assert not (root / ".DS_Store").exists()
        assert (root / "cargo-home" / "credentials.toml").is_file()
        assert any(reason == "contains Cargo credentials" for _path, reason in skip)

    with tempfile.TemporaryDirectory(prefix="ca-clean-") as temporary:
        root = Path(temporary)
        target = root / "target"
        (target / "debug" / ".fingerprint").mkdir(parents=True)
        (target / "debug" / "incremental").mkdir()
        (target / ".rustc_info.json").write_text("{}\n")
        (target / "debug" / "ca").write_text("binary\n")
        (root / "docs" / "target" / "notes.md").parent.mkdir(parents=True)
        (root / "docs" / "target" / "notes.md").write_text("keep\n")
        remove, _skip = plan(root, keep_scratch=False, keep_target=False)
        assert target in {item.path for item in remove}
        assert not execute(remove)
        assert not target.exists()
        assert (root / "docs" / "target" / "notes.md").is_file()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true", help="Print paths without deleting them")
    parser.add_argument("--keep-scratch", action="store_true", help="Keep scratch task trees; still remove caches inside them")
    parser.add_argument("--keep-target", action="store_true", help="Keep this repository's Cargo target directory")
    parser.add_argument("--self-test", action="store_true", help="Run fixture checks in a temporary directory")
    args = parser.parse_args(argv)
    if args.self_test:
        self_test()
        print("self-test passed")
        return 0
    root = repository_root()
    remove, skip = plan(root, keep_scratch=args.keep_scratch, keep_target=args.keep_target)
    try:
        if args.dry_run:
            report(root, remove, skip, dry_run=True)
            return 0
        failed = execute(remove)
        report(root, remove, skip, dry_run=False)
        for path, error in failed:
            print(f"failed {display(root, path)}: {error}", file=sys.stderr)
        return 1 if failed else 0
    finally:
        if args.dry_run:
            for item in remove:
                close_locks(item.locks)


if __name__ == "__main__":
    sys.exit(main())
