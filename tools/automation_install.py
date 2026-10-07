#!/usr/bin/env python3
"""Verify and recoverably install the paired ca/codex-team release.

--check does not stage or replace files. Installation keeps private, same-device
backups and a durable receipt. --recover RECEIPT restores both before-images,
including after interruption; an unrelated destination change requires review.
No config, vault, session, service or existing process is changed.
"""

import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time


SCHEMA = "ca.release_pair.v1"
NAMES = ("ca", "codex-team")
OUTPUT_LIMIT = 1024 * 1024
FILE_LIMIT = 1024 * 1024 * 1024
INSTALL_LEASE_FDS = ()


class InstallError(RuntimeError):
    pass


def require(condition, reason):
    if not condition:
        raise InstallError(reason)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def sync_directory(path):
    descriptor = os.open(str(path), os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def fingerprint(path):
    """Hash the same bounded, non-symlink inode whose metadata is recorded."""
    path = Path(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode), f"not a regular executable: {path}")
    require(before.st_uid == os.getuid(), f"unexpected executable owner: {path}")
    require(before.st_mode & stat.S_IXUSR, f"not owner-executable: {path}")
    require(not before.st_mode & 0o6022, f"unsafe executable mode: {path}")
    require(before.st_size <= FILE_LIMIT, f"executable exceeds one GiB: {path}")
    descriptor = os.open(str(path), os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        opened = os.fstat(descriptor)
        require((opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino),
                f"executable changed while opening: {path}")
        hasher = hashlib.sha256()
        total = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            require(total <= FILE_LIMIT, f"executable grew beyond its bound: {path}")
            hasher.update(chunk)
        after = os.fstat(descriptor)
        current = path.lstat()
        fields = ("st_dev", "st_ino", "st_size", "st_uid", "st_gid", "st_mode",
                  "st_mtime_ns", "st_ctime_ns", "st_nlink")
        require(all(getattr(before, field) == getattr(after, field)
                    == getattr(current, field) for field in fields),
                f"executable changed while hashing: {path}")
        return {
            "device": before.st_dev, "inode": before.st_ino, "size": total,
            "uid": before.st_uid, "gid": before.st_gid,
            "mode": stat.S_IMODE(before.st_mode), "links": before.st_nlink,
            "mtime_ns": before.st_mtime_ns, "ctime_ns": before.st_ctime_ns,
            "sha256": hasher.hexdigest(),
        }
    finally:
        os.close(descriptor)


def stable(path, expected):
    require(fingerprint(path) == expected, f"concurrent executable change: {path}")


def run_checked(arguments, timeout=20, return_stderr=False, argv0=None):
    """Bound output/time and clean up only this verification child group."""
    command = [str(value) for value in arguments]
    executable = None
    if argv0 is not None:
        require(argv0 in NAMES, "verification argv0 is not a paired executable name")
        executable = command[0]
        require(Path(executable).is_absolute(), "verification executable must be absolute")
        command[0] = argv0
    with tempfile.TemporaryDirectory(prefix="ca-release-verification-") as root:
        environment = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": root,
            "XDG_CONFIG_HOME": root + "/config", "XDG_DATA_HOME": root + "/data",
            "XDG_CACHE_HOME": root + "/cache", "TMPDIR": root, "LC_ALL": "C",
            "TERM": "dumb", "NO_COLOR": "1",
        }
        child = subprocess.Popen(command, executable=executable, cwd=root,
                                 env=environment, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=True, pass_fds=INSTALL_LEASE_FDS)
        selector = selectors.DefaultSelector()
        output = {"stdout": bytearray(), "stderr": bytearray()}
        for stream, name in ((child.stdout, "stdout"), (child.stderr, "stderr")):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, name)
        deadline = time.monotonic() + timeout
        try:
            while selector.get_map():
                require(time.monotonic() < deadline,
                        f"verification timed out: {Path(arguments[0]).name}")
                for key, _ in selector.select(max(0.001, min(0.1, deadline - time.monotonic()))):
                    chunk = os.read(key.fileobj.fileno(), 8192)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        key.fileobj.close()
                    else:
                        output[key.data].extend(chunk)
                        require(sum(len(value) for value in output.values()) <= OUTPUT_LIMIT,
                                "verification output exceeds one MiB")
            status = child.wait(timeout=max(0.01, deadline - time.monotonic()))
            diagnostic = "".join(character for character in
                output["stderr"].decode("utf-8", "replace") if character.isprintable())[:512]
            require(status == 0, f"verification failed ({status}): "
                    f"{Path(arguments[0]).name}; {diagnostic}; "
                    f"stderr SHA-256 {digest(output['stderr'])}")
            return bytes(output["stderr"] if return_stderr else output["stdout"])
        finally:
            selector.close()
            # A helper must not leave descendants holding files or pipes.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            for stream in (child.stdout, child.stderr):
                stream.close()


def signature(path, sign=False):
    if sys.platform == "darwin":
        if sign:
            run_checked(["/usr/bin/codesign", "--force", "--sign", "-", path])
        run_checked(["/usr/bin/codesign", "--verify", "--strict", "--all-architectures",
                     "--verbose=3", path])


def code_directory_identity(path):
    if sys.platform != "darwin":
        return None
    signature(path)
    display = run_checked(["/usr/bin/codesign", "--display", "--verbose=4", path],
                          return_stderr=True).decode("utf-8", "strict")
    format_line = re.search(r"^Format=Mach-O (?:thin|universal) \(([A-Za-z0-9_ ]+)\)$",
                            display, re.MULTILINE)
    require(format_line is not None, "Mach-O architecture identity is unavailable")
    architectures = format_line.group(1).split()
    require(1 <= len(architectures) <= 16 and len(set(architectures)) == len(architectures),
            "Mach-O architecture count is invalid")
    identity = {"ad_hoc": True, "architectures": {}}
    for architecture in architectures:
        require(len(architecture) <= 32, "Mach-O architecture name exceeds its bound")
        details = run_checked(["/usr/bin/codesign", "--display", "--verbose=4",
                               "--architecture", architecture, path],
                              return_stderr=True).decode("utf-8", "strict")
        full = sorted(set(re.findall(r"^CandidateCDHashFull sha256=([0-9a-fA-F]{64})$",
                                    details, re.MULTILINE)))
        require(full, "full SHA-256 CodeDirectory identity is unavailable")
        identity["architectures"][architecture] = [value.lower() for value in full]
        identity["ad_hoc"] = identity["ad_hoc"] and bool(
            re.search(r"^Signature=adhoc$", details, re.MULTILINE))
    return identity


def inspect(path, name, new_release=False):
    before = fingerprint(path)
    signature(path)
    code_directory = code_directory_identity(path)
    if new_release and sys.platform == "darwin":
        require(code_directory["ad_hoc"], f"release must be pre-signed ad-hoc: {name}")
    # Clap derives Usage from argv0. Backups deliberately have recovery names,
    # so probe the exact inspected image using its stable public command name.
    version = run_checked([path, "--version"], argv0=name).decode("ascii").strip()
    match = re.fullmatch(re.escape(name) + r" ([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)",
                         version)
    require(match is not None, f"unexpected version response: {name}")
    help_output = run_checked([path, "--help"], argv0=name)
    require(b"Usage:" in help_output and name.encode() in help_output,
            f"missing CLI help: {name}")
    interfaces = {}
    if new_release:
        if name == "ca":
            arguments = ["automation-engine", "--help"]
            required = (b"preview", b"start", b"status", b"cancel", b"reconcile")
        else:
            arguments = ["automation", "--help"]
            required = (b"workflow", b"workflow-roots", b"setup")
        typed_help = run_checked([path, *arguments], argv0=name)
        require(all(value in typed_help for value in required),
                f"missing native/compatibility automation commands: {name}")
        interfaces[" ".join(arguments)] = digest(typed_help)
    stable(path, before)
    return {"file": before, "version": version, "semver": match.group(1),
            "help_sha256": digest(help_output), "interfaces": interfaces,
            "code_directory": code_directory,
            "signature": "strict_codesign" if sys.platform == "darwin" else "not_applicable"}


def verify(path, name, expected, new_release=False):
    actual = inspect(path, name, new_release)
    for field in ("sha256", "size", "mode", "uid"):
        require(actual["file"][field] == expected["file"][field],
                f"copied executable {field} mismatch: {name}")
    for field in ("version", "help_sha256", "interfaces", "signature"):
        require(actual[field] == expected[field], f"copied executable {field} mismatch: {name}")
    if "code_directory" in expected:
        require(actual["code_directory"] == expected["code_directory"],
                f"copied executable CodeDirectory mismatch: {name}")
    return actual["file"]


def path_binding(name, destination):
    resolved = shutil.which(name)
    require(resolved is not None, f"existing {name} executable is missing from PATH")
    require(not Path(resolved).is_symlink(), f"PATH executable is a symlink: {name}")
    require(Path(resolved).resolve(strict=True) == Path(destination),
            f"PATH destination changed: {name}")


def parent_identity(destination):
    parent = Path(destination).parent
    info = parent.lstat()
    require(stat.S_ISDIR(info.st_mode) and parent.resolve(strict=True) == parent,
            "destination parent is no longer its canonical directory")
    return {"device": info.st_dev, "inode": info.st_ino, "uid": info.st_uid,
            "gid": info.st_gid, "mode": stat.S_IMODE(info.st_mode)}


def entry_path_binding(entry):
    path_binding(entry["name"], entry["destination"])
    require(parent_identity(entry["destination"]) == entry["destination_parent"],
            f"destination parent changed: {entry['name']}")


def private_directory(path):
    info = Path(path).lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.getuid()
            and not info.st_mode & 0o077, f"recovery directory is not private: {path}")


def copy_verified(source, destination, expected):
    stable(source, expected)
    source_fd = os.open(str(source), os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    destination_fd = os.open(str(destination), os.O_WRONLY | os.O_CREAT | os.O_EXCL
                             | getattr(os, "O_NOFOLLOW", 0), 0o600)
    try:
        with os.fdopen(source_fd, "rb", closefd=False) as incoming:
            with os.fdopen(destination_fd, "wb", closefd=False) as outgoing:
                shutil.copyfileobj(incoming, outgoing, 1024 * 1024)
                outgoing.flush()
        os.fchmod(destination_fd, expected["mode"])
        os.fsync(destination_fd)
    finally:
        os.close(source_fd)
        os.close(destination_fd)
    os.utime(destination, ns=(expected["mtime_ns"], expected["mtime_ns"]),
             follow_symlinks=False)
    stable(source, expected)
    copied = fingerprint(destination)
    require(all(copied[key] == expected[key] for key in ("sha256", "size", "mode", "uid")),
            f"staged copy does not match its before-image: {destination}")


def write_receipt(path, receipt):
    encoded = (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode()
    require(len(encoded) <= OUTPUT_LIMIT, "installation receipt exceeds one MiB")
    descriptor, temporary = tempfile.mkstemp(prefix=".receipt-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        sync_directory(path.parent)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def acquire_locks(entries):
    locks = []
    try:
        for entry in sorted(entries, key=lambda value: value["destination"]):
            destination = Path(entry["destination"])
            path = destination.parent / ("." + entry["name"] + "-automation-install.lock")
            descriptor = os.open(str(path), os.O_RDWR | os.O_CREAT
                                 | getattr(os, "O_NOFOLLOW", 0), 0o600)
            locks.append(descriptor)
            info = os.fstat(descriptor)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                    and not info.st_mode & 0o077 and info.st_nlink == 1,
                    f"installation lock has unexpected ownership: {path}")
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return locks
    except BaseException:
        for descriptor in locks:
            os.close(descriptor)
        raise


def renamed_match(actual, expected):
    # Atomic rename can change timestamps, but not the inode or artifact bytes.
    return expected is not None and all(actual[key] == expected[key] for key in
        ("device", "inode", "size", "uid", "gid", "mode", "links", "sha256"))


def signing_readback(entry, current):
    intent = entry.get("signing")
    require(intent is not None and intent.get("pending_reconciliation") is True,
            "changed artifact has no interrupted signing intent")
    entry_path_binding(entry)
    require(all(current[key] == intent["file"][key] for key in ("uid", "gid", "mode"))
            and current["links"] == 1 and current["device"] == intent["file"]["device"],
            "signing changed executable ownership, mode or filesystem")
    identity = code_directory_identity(entry["destination"])
    require(identity is not None and identity["ad_hoc"]
            and identity == intent["code_directory"],
            "interrupted signer produced a different or unverifiable CodeDirectory")
    stable(entry["destination"], current)
    observed = Path(entry["stage_directory"]) / (
        entry["name"] + ".observed-signed-" + current["sha256"])
    if not observed.exists():
        copy_verified(Path(entry["destination"]), observed, current)
    copied = fingerprint(observed)
    require(all(copied[key] == current[key] for key in ("sha256", "size", "uid", "mode")),
            "observed signing after-image changed")
    require(code_directory_identity(observed) == identity,
            "observed signing copy lost its strict CodeDirectory identity")
    intent["observed_after_image"] = {"path": str(observed), "file": copied}
    intent["pending_reconciliation"] = False
    intent["phase"] = "reconciled"
    entry["installed_file"] = current


def rollback(receipt, receipt_path):
    errors = []
    signals = {caught: signal.getsignal(caught)
               for caught in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    for caught in signals:
        signal.signal(caught, signal.SIG_IGN)

    def retain_progress():
        try:
            write_receipt(receipt_path, receipt)
        except BaseException as error:
            errors.append(f"recovery receipt: {error}")

    interrupted_phase = receipt["phase"]
    for entry in receipt["entries"]:
        intent = entry.get("signing")
        if intent is not None and interrupted_phase == "signing_" + entry["name"]:
            intent["pending_reconciliation"] = intent.get("phase") == "running"
    receipt["phase"] = "rolling_back"
    retain_progress()
    for entry in reversed(receipt["entries"]):
        try:
            destination = Path(entry["destination"])
            entry_path_binding(entry)
            current = fingerprint(destination)
            if (current == entry["before"]["file"] or current == entry.get("restored_file")
                    or renamed_match(current, entry.get("rollback_staged_file"))):
                verify(destination, entry["name"], entry["before"])
                entry["restored_file"] = current
                retain_progress()
                continue
            owned = entry.get("installed_file") or entry.get("staged_file")
            if not renamed_match(current, owned):
                signing_readback(entry, current)
                retain_progress()
            backup = Path(entry["backup"])
            verify(backup, entry["name"], entry["before"])
            restore = Path(entry["rollback_staged"])
            if not restore.exists():
                copy_verified(backup, restore, fingerprint(backup))
                entry["rollback_staged_file"] = fingerprint(restore)
                retain_progress()
            stable(restore, entry["rollback_staged_file"])
            verify(restore, entry["name"], entry["before"])
            entry_path_binding(entry)
            stable(destination, current)
            os.replace(restore, destination)
            sync_directory(destination.parent)
            # Preserve the exact already verified prior signature and bytes.
            entry["restored_file"] = verify(destination, entry["name"], entry["before"])
            retain_progress()
        except BaseException as error:
            errors.append(f"{entry['name']}: {error}")
    receipt["phase"] = "needs_attention" if errors else "rolled_back"
    receipt["recovery_errors"] = errors
    retain_progress()
    for caught, handler in signals.items():
        signal.signal(caught, handler)
    require(not errors, "paired rollback requires attention: " + "; ".join(errors))


def install(entries):
    global INSTALL_LEASE_FDS
    locks = acquire_locks(entries)
    INSTALL_LEASE_FDS = tuple(locks)
    receipt_path = None
    receipt = None
    try:
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
        for entry in entries:
            destination = Path(entry["destination"])
            entry_path_binding(entry)
            stable(destination, entry["before"]["file"])
            stage = Path(tempfile.mkdtemp(prefix="." + entry["name"] + "-automation-stage-",
                                          dir=destination.parent))
            private_directory(stage)
            entry["stage_directory"] = str(stage)
            backup = stage / (entry["name"] + ".recovery-automation-" + stamp + "-"
                              + entry["before"]["file"]["sha256"])
            candidate = stage / entry["name"]
            restore = stage / (entry["name"] + ".restore")
            entry["backup"], entry["staged"] = str(backup), str(candidate)
            entry["rollback_staged"] = str(restore)
            copy_verified(destination, backup, entry["before"]["file"])
            verify(backup, entry["name"], entry["before"])
            copy_verified(destination, restore, entry["before"]["file"])
            entry["rollback_staged_file"] = verify(restore, entry["name"], entry["before"])
            copy_verified(Path(entry["candidate"]), candidate, entry["after"]["file"])
            signature(candidate, sign=True)
            entry["staged_file"] = verify(candidate, entry["name"], entry["after"], True)
            require(entry["staged_file"]["device"] == entry["before"]["file"]["device"],
                    "release staging is not on the destination filesystem")
            sync_directory(stage)
        receipt = {"schema": SCHEMA, "uid": os.getuid(), "created_at": stamp,
                   "phase": "prepared", "entries": entries}
        receipt_path = Path(entries[0]["stage_directory"]) / "paired-install.json"
        write_receipt(receipt_path, receipt)
        print(f"Recovery receipt: {receipt_path}", flush=True)
        for entry in entries:
            # Refuse a change to either member, including between replacements.
            for member in entries:
                entry_path_binding(member)
                stable(member["destination"], member.get("installed_file", member["before"]["file"]))
            stable(entry["candidate"], entry["after"]["file"])
            verify(entry["staged"], entry["name"], entry["after"], True)
            receipt["phase"] = "replacing_" + entry["name"]
            write_receipt(receipt_path, receipt)
            destination = Path(entry["destination"])
            entry_path_binding(entry)
            stable(destination, entry["before"]["file"])
            os.replace(entry["staged"], destination)
            sync_directory(destination.parent)
            entry["installed_file"] = fingerprint(destination)
            entry["signing"] = {"phase": "running", "file": entry["installed_file"],
                                "code_directory": entry["after"]["code_directory"],
                                "pending_reconciliation": True}
            receipt["phase"] = "signing_" + entry["name"]
            write_receipt(receipt_path, receipt)
            try:
                signature(destination, sign=True)
            finally:
                signed = fingerprint(destination)
                if not renamed_match(signed, entry["installed_file"]):
                    signing_readback(entry, signed)
                else:
                    entry["installed_file"] = signed
                    entry["signing"]["pending_reconciliation"] = False
                    entry["signing"]["phase"] = "confirmed"
                write_receipt(receipt_path, receipt)
            entry["installed_file"] = verify(destination, entry["name"], entry["after"], True)
            write_receipt(receipt_path, receipt)
        for entry in entries:
            entry_path_binding(entry)
            stable(entry["destination"], entry["installed_file"])
            verify(entry["destination"], entry["name"], entry["after"], True)
        receipt["phase"] = "completed"
        write_receipt(receipt_path, receipt)
        return receipt
    except BaseException:
        if receipt_path is not None:
            rollback(receipt, receipt_path)
        else:
            print("No executable was replaced; any prepared recovery stages are retained.",
                  file=sys.stderr)
            for entry in entries:
                if "stage_directory" in entry:
                    print(entry["stage_directory"], file=sys.stderr)
        raise
    finally:
        INSTALL_LEASE_FDS = ()
        for descriptor in locks:
            os.close(descriptor)


def recover(path):
    global INSTALL_LEASE_FDS
    path = Path(path).absolute()
    private_directory(path.parent)
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
            and not info.st_mode & 0o077 and info.st_size <= OUTPUT_LIMIT,
            "recovery receipt is not a bounded private regular file")
    receipt = json.loads(path.read_bytes())
    require(receipt.get("schema") == SCHEMA and receipt.get("uid") == os.getuid()
            and [entry["name"] for entry in receipt["entries"]] == list(NAMES),
            "recovery receipt identity is invalid")
    for entry in receipt["entries"]:
        stage, destination = Path(entry["stage_directory"]), Path(entry["destination"])
        private_directory(stage)
        require(stage.resolve(strict=True) == stage and destination.is_absolute()
                and stage.parent == destination.parent
                and Path(entry["backup"]).parent == stage
                and Path(entry["rollback_staged"]) == stage / (entry["name"] + ".restore")
                and Path(entry["staged"]) == stage / entry["name"],
                "recovery paths do not match the paired installation")
    require(path == Path(receipt["entries"][0]["stage_directory"]) / "paired-install.json",
            "recovery receipt moved outside its owning stage")
    locks = acquire_locks(receipt["entries"])
    INSTALL_LEASE_FDS = tuple(locks)
    try:
        rollback(receipt, path)
    finally:
        INSTALL_LEASE_FDS = ()
        for descriptor in locks:
            os.close(descriptor)
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify the pair without staging or replacement")
    mode.add_argument("--recover", type=Path, metavar="RECEIPT", help="restore both verified before-images")
    repository = Path(__file__).resolve().parent.parent
    for name in NAMES:
        parser.add_argument("--" + name + "-candidate", type=Path,
                            default=repository / "target" / "release" / name)
    arguments = parser.parse_args()
    if arguments.recover:
        result = recover(arguments.recover)
    else:
        entries = []
        for name in NAMES:
            candidate = getattr(arguments, name.replace("-", "_") + "_candidate").absolute()
            require(not candidate.is_symlink(), f"candidate is a symlink: {name}")
            candidate = candidate.resolve(strict=True)
            found = shutil.which(name)
            require(found is not None, f"existing {name} executable is missing from PATH")
            require(not Path(found).is_symlink(), f"PATH executable is a symlink: {name}")
            destination = Path(found).resolve(strict=True)
            require(candidate != destination, f"candidate is already the installed path: {name}")
            before, after = inspect(destination, name), inspect(candidate, name, True)
            require(before["file"]["links"] == 1, f"installed executable has multiple hard links: {name}")
            entries.append({"name": name, "candidate": str(candidate), "destination": str(destination),
                            "destination_parent": parent_identity(destination),
                            "before": before, "after": after})
        require(entries[0]["after"]["semver"] == entries[1]["after"]["semver"],
                "candidate ca and codex-team versions differ")
        require(len({entry["destination"] for entry in entries}) == 2,
                "paired executables resolve to the same destination")
        for entry in entries:
            entry_path_binding(entry)
            stable(entry["destination"], entry["before"]["file"])
            stable(entry["candidate"], entry["after"]["file"])
        result = {"schema": SCHEMA, "phase": "checked", "entries": entries} if arguments.check else install(entries)
    print(json.dumps(result, indent=2, sort_keys=True))


def interrupted(signum, _frame):
    raise InstallError(f"installation interrupted by signal {signum}")


if __name__ == "__main__":
    for caught in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(caught, interrupted)
    try:
        main()
    except (Exception, KeyboardInterrupt) as failure:
        print(f"Paired release installation failed: {failure}", file=sys.stderr)
        sys.exit(1)
