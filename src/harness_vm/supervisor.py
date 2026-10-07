"""Host-owned Tart lease. EOF on the owner's pipe stops the entire VM.

No guest-supplied paths or commands enter this controller. The embedding Rust
adapter validates and freezes the profile before starting it.
"""
import json
import os
import select
import signal
import subprocess
import sys
import tempfile
import time
import uuid


class LeaseEnded(Exception):
    pass


def terminate_owned(child):
    if child.poll() is None:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            # Darwin can report either error while a group is disappearing.
            # Never infer termination from the error: a bounded wait must
            # confirm this exact owned child exited, or cleanup fails closed.
            pass
    child.wait(timeout=10)


class Supervisor:
    def __init__(self, config):
        self.config = config
        self.name = "ca-harness-run-" + uuid.uuid4().hex
        self.deadline = time.monotonic() + config["seconds"]
        self.ending = False
        self.owned = False
        self.runner = None
        self.scratch = tempfile.TemporaryDirectory(prefix="ca-vm-supervisor-")
        self.env = {
            "HOME": self.scratch.name,
            "TMPDIR": self.scratch.name,
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "TART_HOME": config["profile"]["home"],
            "TART_NO_AUTO_PRUNE": "1",
        }

    def check_owner(self):
        if self.ending or time.monotonic() >= self.deadline:
            raise LeaseEnded("VM lease ended")
        if select.select([sys.stdin.fileno()], [], [], 0)[0]:
            # After the initial config, closing or writing the control pipe is
            # a stop request. Never interpret subsequent bytes as commands.
            os.read(sys.stdin.fileno(), 1)
            raise LeaseEnded("VM owner released lease")

    def command(self, args, seconds=30, cleanup=False):
        with tempfile.TemporaryFile() as output:
            child = subprocess.Popen(
                [self.config["profile"]["tart"], *args],
                env=self.env, cwd=self.scratch.name,
                stdin=subprocess.DEVNULL, stdout=output,
                stderr=subprocess.DEVNULL, start_new_session=True,
            )
            deadline = time.monotonic() + seconds
            try:
                while True:
                    if not cleanup:
                        self.check_owner()
                    if os.fstat(output.fileno()).st_size > 1024 * 1024:
                        raise RuntimeError("Tart output exceeded its bound")
                    status = child.poll()
                    if status is not None:
                        break
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Tart command timed out")
                    time.sleep(0.02)
                if status:
                    raise RuntimeError("Tart command failed")
                output.seek(0)
                data = output.read(1024 * 1024 + 1)
                if len(data) > 1024 * 1024:
                    raise RuntimeError("Tart output exceeded its bound")
                return data
            finally:
                terminate_owned(child)

    def state(self, name, cleanup=False):
        rows = json.loads(self.command(
            ["list", "--source", "local", "--format", "json"], cleanup=cleanup))
        matches = [r for r in rows if r["Name"] == name and r["Source"] == "local"]
        if len(matches) != 1:
            raise RuntimeError("VM inventory is missing or ambiguous")
        return matches[0]

    def verify_template(self):
        profile = self.config["profile"]
        for name, saved in self.config["files"].items():
            path = (profile["tart"] if name == "tart" else
                    os.path.join(profile["home"], "vms", profile["template"], name))
            current = os.stat(path, follow_symlinks=False)
            observed = {
                "device": current.st_dev, "inode": current.st_ino,
                "bytes": current.st_size, "uid": current.st_uid,
                "mode": current.st_mode & 0o777,
                "modified_seconds": current.st_mtime_ns // 1000000000,
                "modified_nanoseconds": current.st_mtime_ns % 1000000000,
                "changed_seconds": current.st_ctime_ns // 1000000000,
                "changed_nanoseconds": current.st_ctime_ns % 1000000000,
            }
            if any(saved[k] != v for k, v in observed.items()):
                raise RuntimeError("Frozen VM input changed")
        row = self.state(profile["template"])
        if row["Running"] or row["State"] != "stopped":
            raise RuntimeError("VM template is running")

    def run(self):
        self.verify_template()
        # Mark ownership before clone: a failed/interrupted clone can leave a
        # partial directory. The fresh UUID name never refers to a shared VM.
        if os.path.lexists(os.path.join(self.env["TART_HOME"], "vms", self.name)):
            raise RuntimeError("Disposable VM name already exists")
        self.owned = True
        self.command(["clone", self.config["profile"]["template"], self.name], 120)
        self.verify_template()
        self.check_owner()
        self.runner = subprocess.Popen(
            [self.config["profile"]["tart"], "run", "--no-graphics", "--no-audio",
             "--no-clipboard", self.name],
            env=self.env, cwd=self.scratch.name, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        boot_deadline = min(self.deadline, time.monotonic() + 120)
        while True:
            self.check_owner()
            if self.runner.poll() is not None:
                raise RuntimeError("Disposable VM exited before readiness")
            row = self.state(self.name)
            if row["Running"] and row["State"] == "running":
                break
            if time.monotonic() >= boot_deadline:
                raise RuntimeError("Disposable VM boot timed out")
            time.sleep(0.1)
        print(json.dumps({"ready": True, "name": self.name}), flush=True)
        while True:
            self.check_owner()
            if self.runner.poll() is not None:
                raise RuntimeError("Disposable VM exited during its lease")
            time.sleep(0.05)

    def cleanup(self):
        if not self.owned:
            return True
        try:
            # Even when the owner or its process group dies, this independently
            # sessioned supervisor holds the Tart handle and stops the guest.
            try:
                self.command(["stop", self.name, "--timeout", "0"], cleanup=True)
            except Exception:
                pass  # Inventory below, not stop's exit code, decides cleanup.
            if self.runner is not None:
                terminate_owned(self.runner)
            row = self.state(self.name, cleanup=True)
            if row["Running"] or row["State"] != "stopped":
                return False
            self.command(["delete", self.name], cleanup=True)
            return not os.path.lexists(os.path.join(self.env["TART_HOME"], "vms", self.name))
        except Exception:
            return False


def main():
    # Read one byte at a time so buffered input cannot hide an owner stop byte.
    data = bytearray()
    while len(data) <= 65536:
        value = os.read(sys.stdin.fileno(), 1)
        if not value:
            raise RuntimeError("VM owner closed before configuration")
        if value == b"\n":
            break
        data.extend(value)
    else:
        raise RuntimeError("VM configuration exceeded its bound")
    config = json.loads(data)
    if not isinstance(config["seconds"], int) or not 1 <= config["seconds"] <= 7200:
        raise RuntimeError("Invalid VM lease deadline")
    supervisor = Supervisor(config)
    def ending(_signal, _frame):
        supervisor.ending = True
    signal.signal(signal.SIGTERM, ending)
    signal.signal(signal.SIGINT, ending)
    error = None
    try:
        supervisor.run()
    except LeaseEnded:
        pass
    except Exception as exc:
        error = str(exc)
    finally:
        cleaned = supervisor.cleanup()
        print(json.dumps({"stopped": cleaned, "name": supervisor.name, "error": error}), flush=True)
        supervisor.scratch.cleanup()
    return 0 if cleaned and error is None else 1


if __name__ == "__main__":
    sys.exit(main())
