#!/usr/bin/env python3
"""Run required CA checks and retain independently evaluable evidence."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
EXIT = {"passed": 0, "failed": 1, "infrastructure_failed": 2}
GROUP_TERMINATION_GRACE = 15


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def source_inventory(root):
    """Cover source and verification criteria, including untracked inputs."""
    names = ["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml",
             "build_support", "src", "crates", "tests", "prompts", "scripts",
             "tools", ".cargo", ".codex/hooks.json"]
    files = {}
    for name in names:
        entry = root / name
        paths = [entry] if entry.is_file() else entry.rglob("*") if entry.is_dir() else []
        for path in paths:
            relative = path.relative_to(root)
            if any(part in {"target", "__pycache__", ".git"} for part in relative.parts):
                continue
            if path.is_symlink():
                raise ValueError("verification input is a symlink: " + str(relative))
            if path.is_file():
                files[str(relative)] = hashlib.sha256(path.read_bytes()).hexdigest()
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest(), files


def isolated_environment(output):
    # Never copy ambient application/provider credentials or global Git config.
    original_home = Path.home()
    home = output / "home"
    # Keep socket paths short, but outside /tmp: ordinary jobs intentionally
    # allow /tmp writes and some ownership fixtures claim that entire tree.
    tmp_parent = "/private/var/tmp" if Path("/private/var/tmp").is_dir() else "/var/tmp"
    tmp = Path(tempfile.mkdtemp(prefix="ca-v-", dir=tmp_parent)).resolve()
    for path in (home, tmp, home / ".config", home / ".local/share", home / ".cache"):
        path.mkdir(parents=True, exist_ok=True, mode=0o700)
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home),
           "XDG_CONFIG_HOME": str(home / ".config"),
           "XDG_DATA_HOME": str(home / ".local/share"),
           "XDG_CACHE_HOME": str(home / ".cache"), "TMPDIR": str(tmp),
           "CARGO_HOME": str(home / ".cargo"),
           "RUSTUP_HOME": str(Path(os.environ.get("RUSTUP_HOME", original_home / ".rustup")).resolve()),
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
           "GIT_TERMINAL_PROMPT": "0", "CARGO_NET_OFFLINE": "true",
           "CARGO_TERM_COLOR": "never", "TERM": "xterm-256color",
           "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8",
           "INSTA_UPDATE": "no", "RUST_BACKTRACE": "1", "PYTHONDONTWRITEBYTECODE": "1"}
    # Copy dependency bytes, without the user's Cargo config/credentials. Native
    # isolation deliberately requires a real registry directory, and private
    # files prevent fixture writes from changing the user's dependency cache.
    cache = Path(os.environ.get("CARGO_HOME", original_home / ".cargo")).resolve()
    private_cache = home / ".cargo"
    private_cache.mkdir(mode=0o700)
    for name in ("registry", "git"):
        if (cache / name).exists():
            shutil.copytree(cache / name, private_cache / name)
    # The cache is reusable; application runtime state and config are disposable.
    # Preserve explicit compiler/linker controls without recording their values.
    for key in ("RUSTC", "RUSTDOC", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CC", "CXX", "AR",
                "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS"):
        if key in os.environ:
            env[key] = os.environ[key]
    return env


def classify(returncode, data, kind, timed_out=False):
    if returncode == 0:
        return "passed"
    if timed_out or returncode < 0:
        return "infrastructure_failed"
    if kind == "evaluate":
        return "infrastructure_failed" if returncode == 2 else "failed"
    infra = ("No space left on device", "failed to download", "failed to get `",
             "no matching package named", "Cargo registry cache must be a real directory",
             "Permission denied (os error", "Operation not permitted (os error",
             "could not find `Cargo.toml`", "failed to execute process")
    if kind == "runtime":
        return "infrastructure_failed" if returncode == 2 else "failed"
    return "infrastructure_failed" if any(text in data for text in infra) else "failed"


def process_group_exists(group):
    try:
        os.killpg(group, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        # On macOS an exited, not-yet-reaped member can temporarily make the
        # existence probe return EPERM. Keep draining until the parent is reaped.
        return True


def signal_owned_group(group, signum):
    try:
        os.killpg(group, signum)
    except ProcessLookupError:
        pass


def terminate_owned_group(process, grace_seconds=None):
    """Terminate all stage descendants even when their parent exits first."""
    grace_seconds = GROUP_TERMINATION_GRACE if grace_seconds is None else grace_seconds
    group = process.pid  # start_new_session=True makes the owned leader its PGID.
    signal_owned_group(group, signal.SIGTERM)
    deadline = time.monotonic() + grace_seconds
    try:
        while True:
            process.poll()  # Reap the parent without mistaking exit for group completion.
            if not process_group_exists(group):
                break
            if time.monotonic() >= deadline:
                break
            time.sleep(min(0.05, max(0, deadline - time.monotonic())))
    finally:
        # This also handles a second interruption during the graceful drain.
        process.poll()
        if process_group_exists(group):
            signal_owned_group(group, signal.SIGKILL)
        process.wait()
        # Let delivered KILLs finish before hashing logs. Orphan zombies may
        # remain briefly until init reaps them; they cannot write evidence.
        deadline = time.monotonic() + 1
        while process_group_exists(group) and time.monotonic() < deadline:
            time.sleep(0.01)


def run_stage(stage_id, command, output, env, timeout, kind="check"):
    log = output / (stage_id + ".log")
    started = time.monotonic()
    timed_out = False
    print("[verify] " + stage_id + " started", flush=True)
    with log.open("wb") as stream:
        try:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
                                       stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                returncode = process.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                timed_out = True
                terminate_owned_group(process)
                returncode = 124
                stream.write(b"\n[verify] Stage deadline/interruption; owned process group terminated.\n")
        except OSError as error:
            returncode = 127
            stream.write(("[verify] launch failed: " + str(error) + "\n").encode())
    data = log.read_text(errors="replace")
    status = "infrastructure_failed" if returncode == 127 else classify(returncode, data, kind, timed_out)
    result = {"id": stage_id, "command": command, "kind": kind, "status": status,
              "returncode": returncode, "duration_seconds": round(time.monotonic() - started, 3),
              "log": str(log), "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()}
    print("[verify] " + stage_id + ": " + status + " (" + str(result["duration_seconds"]) + "s)", flush=True)
    if status != "passed":
        print(data[-5000:], file=sys.stderr, flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="New evidence directory (default scratch/verify/<unique run>)")
    parser.add_argument("--baseline", type=Path, help="Prior passing report for runtime metric/test-count comparison")
    parser.add_argument("--timeout", type=int, default=3600, help="Per-stage deadline in seconds (default 3600)")
    args = parser.parse_args()
    if args.timeout < 1:
        parser.error("--timeout must be positive")
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    output = (args.output or ROOT / "scratch/verify" / (stamp + "-" + str(os.getpid()))).resolve()
    try:
        output.mkdir(parents=True, mode=0o700, exist_ok=False)
    except OSError as error:
        print("[verify] evidence directory unavailable: " + str(error), file=sys.stderr)
        return 2
    report = {"schema": "ca.verify.v1", "status": "running", "complete": False,
              "started_at": stamp, "root": str(ROOT), "output": str(output),
              "platform": platform.platform(), "stages": [], "diagnostics": [],
              "limits": ["Ignored opt-in tests are reported, not executed.",
                         "Loopback fixtures verify local behavior, not paid-provider effectiveness.",
                         "Host-platform validation; no cross-platform or long-duration soak claim.",
                         "Disposable fixtures and scrubbed environment are not OS security containment; native Keychain isolation requires a VM.",
                         "Native workspace ownership uses host coordination locks; no production locks are cleared."]}
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    try:
        digest, inventory = source_inventory(ROOT)
        report["source_digest"] = digest
        criteria = {name: sha for name, sha in inventory.items()
                    if name.startswith(("scripts/verify", "tools/verify", "tools/test_verify"))
                    or name in {"tools/orc258_pty.py", "tools/bug257_pty.py", ".codex/hooks.json"}}
        report["criteria_digest"] = hashlib.sha256(json.dumps(criteria, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        if args.baseline:
            report["baseline"] = str(args.baseline.resolve())
        write_json(output / "source-manifest.json", inventory)
        env = isolated_environment(output)
        report["isolation"] = {"home": env["HOME"], "tmp": env["TMPDIR"],
                               "credentials_inherited": False, "cargo_offline": True,
                               "cargo_cache_reused": True, "cargo_cache_private_copy": True,
                               "snapshot_updates": False}
        cargo = shutil.which("cargo", path=env["PATH"])
        if not cargo:
            raise RuntimeError("Cargo is unavailable on PATH")
        commands = [
            ("preflight", [cargo, "metadata", "--locked", "--offline", "--format-version", "1", "--no-deps"], "preflight"),
            ("workflow-tests", [sys.executable, "-m", "unittest", "discover", "-s", "tools", "-p", "test_verify*.py", "-v"], "check"),
            ("format", [cargo, "fmt", "--all", "--", "--check"], "check"),
            ("lint", [cargo, "clippy", "--locked", "--offline", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"], "check"),
            ("test-default", [cargo, "test", "--locked", "--offline", "--workspace", "--no-fail-fast", "--", "--test-threads=1"], "test"),
            ("test-all-features", [cargo, "test", "--locked", "--offline", "--workspace", "--all-features", "--no-fail-fast", "--", "--test-threads=1"], "test"),
            ("release-build", [cargo, "build", "--locked", "--offline", "--release", "--workspace", "--all-features"], "check"),
        ]
        for stage_id, command, kind in commands:
            stage = run_stage(stage_id, command, output, env, args.timeout, kind)
            report["stages"].append(stage)
            write_json(output / "report.json", report)
            if stage["status"] == "infrastructure_failed":
                break
        if len(report["stages"]) == len(commands) and report["stages"][-1]["status"] == "passed":
            candidate = output / "candidate"
            candidate.mkdir(mode=0o700)
            binary = candidate / "ca"
            shutil.copy2(target / "release/ca", binary)
            if sys.platform == "darwin":
                sign = run_stage("candidate-sign", ["/usr/bin/codesign", "--force", "--sign", "-", str(binary)],
                                 output, env, 30)
                report["candidate_sign"] = sign
                if sign["status"] != "passed":
                    raise RuntimeError("Disposable candidate signing failed; inspect " + sign["log"])
            report["binary"] = str(binary)
            report["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
            runtime = run_stage("runtime", [sys.executable, str(ROOT / "tools/verify_runtime.py"),
                                "--binary", str(binary), "--output", str(output / "runtime")],
                                output, env, min(args.timeout, 180), "runtime")
            report["stages"].append(runtime)
            if (output / "runtime/result.json").is_file():
                report["runtime_sha256"] = hashlib.sha256((output / "runtime/result.json").read_bytes()).hexdigest()
        after, after_inventory = source_inventory(ROOT)
        write_json(output / "source-manifest-after.json", after_inventory)
        report["source_digest_after"] = after
        if after != digest:
            report["diagnostics"].append("Source or evaluation criteria changed during verification; rerun the frozen tree.")
        report["complete"] = len(report["stages"]) == len(commands) + 1
        report["status"] = ("infrastructure_failed" if any(s["status"] == "infrastructure_failed" for s in report["stages"])
                            else "failed" if not report["complete"] or after != digest or any(s["status"] != "passed" for s in report["stages"])
                            else "passed")
        write_json(output / "report.json", report)
        command = [sys.executable, str(ROOT / "tools/verify_evaluate.py"), "--report", str(output / "report.json")]
        if args.baseline:
            command += ["--baseline", str(args.baseline.resolve())]
        evaluation = run_stage("evaluate", command, output, env, min(args.timeout, 60), "evaluate")
        report["evaluator"] = {"status": evaluation["status"], "report": str(output / "evaluation.json"),
                               "log": evaluation["log"]}
        if evaluation["status"] != "passed":
            report["status"] = evaluation["status"]
        if (output / "evaluation.json").is_file():
            assessment = json.loads((output / "evaluation.json").read_text())
            report["diagnostics"].extend(assessment.get("diagnostics", []))
            report["test_totals"] = assessment.get("test_totals", {})
            report["metrics"] = assessment.get("metrics", {})
    except (OSError, RuntimeError, ValueError) as error:
        report["status"] = "infrastructure_failed"
        report["diagnostics"].append(str(error))
    report["finished_at"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    # ru_maxrss is a process-child high-water mark, not an isolated stage RSS.
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    report["resources"] = {"children_user_seconds": usage.ru_utime, "children_system_seconds": usage.ru_stime,
                           "children_maxrss_raw": usage.ru_maxrss, "maxrss_units": "bytes" if sys.platform == "darwin" else "KiB"}
    write_json(output / "report.json", report)
    print("[verify] " + report["status"] + ": " + str(output / "report.json"), flush=True)
    return EXIT[report["status"]]


if __name__ == "__main__":
    raise SystemExit(main())
