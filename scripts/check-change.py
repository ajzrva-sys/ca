#!/usr/bin/env python3
"""Focused checks in a stable disposable checkout; full gate: scripts/verify.sh."""
import argparse
import datetime
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = next(p for p in Path(__file__).resolve().parents
            if (p / "Cargo.toml").is_file() and (p / "scripts/verify.py").is_file())
STATE = ROOT / "scratch/check-change"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def unmatched_selectors(log, selectors):
    executed = re.findall(r"^test (\S+) \.\.\. ok$", log, re.M)
    return [selector for selector in selectors if not any(selector in name for name in executed)]


def inventory(verifier, root):
    _, files = verifier.source_inventory(root)
    if (root / "clippy.toml").is_file():
        files["clippy.toml"] = sha(root / "clippy.toml")
    return hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest(), files


def sync_source(root, slot, files):
    """Preserve unchanged mtimes and absolute Cargo source paths across runs."""
    slot.mkdir(parents=True, exist_ok=True)
    for path in slot.rglob("*"):
        if path.is_symlink() and path != slot / "target":
            raise ValueError("unexpected symlink in stable checkout: " + str(path))
        if "target" in path.relative_to(slot).parts:
            continue
        if path.is_file() and str(path.relative_to(slot)) not in files:
            path.unlink()
    changed = 0
    for name, digest in files.items():
        destination = slot / name
        if not destination.is_file() or sha(destination) != digest:
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / name, destination)
            changed += 1
    target = slot / "target"
    if not target.exists():
        target.symlink_to(ROOT / "target", target_is_directory=True)
    if not target.is_symlink() or target.resolve() != (ROOT / "target").resolve():
        raise ValueError("Stable checkout target must point at this repository target")
    return changed


def environment(output, cache):
    home = output / "home"
    home.mkdir(mode=0o700)
    tmp = tempfile.mkdtemp(prefix="ca-c-", dir="/private/var/tmp" if sys.platform == "darwin" else "/var/tmp")
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home),
           "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
           "XDG_CACHE_HOME": str(home / ".cache"), "TMPDIR": tmp, "CARGO_HOME": str(cache),
           "RUSTUP_HOME": str(Path(os.environ.get("RUSTUP_HOME", Path.home() / ".rustup")).resolve()),
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_TERMINAL_PROMPT": "0",
           "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never", "INSTA_UPDATE": "no",
           "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8",
           "RUST_BACKTRACE": "1", "PYTHONDONTWRITEBYTECODE": "1"}
    env.update(RUSTFLAGS="-C debuginfo=0", CARGO_BUILD_JOBS="8")
    for key in ("RUSTC", "RUSTDOC", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CC", "CXX", "AR",
                "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "CARGO_BUILD_JOBS"):
        if key in os.environ:
            env[key] = os.environ[key]
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--selector", action="append", default=[], help="Explicit Rust test name/filter; repeatable")
    parser.add_argument("--prepare-only", action="store_true", help="Prepare and measure the stable checkout/cache without Cargo")
    parser.add_argument("--all-features", action="store_true")
    parser.add_argument("--release", action="store_true", help="Build the installable ca binary with all features once")
    parser.add_argument("--cargo-home", type=Path, default=STATE / "cargo-home", help="Stable private Cargo cache below scratch")
    parser.add_argument("--seed-cargo-home", type=Path, help="Copy only registry/git bytes once; never Cargo settings/credentials")
    args = parser.parse_args()
    if (not args.selector and not args.prepare_only) or any(not item.strip() for item in args.selector):
        parser.error("explicit nonempty --selector required unless --prepare-only")
    prepared_at = time.monotonic()
    cache = args.cargo_home.resolve()
    if not cache.is_relative_to((ROOT / "scratch").resolve()) or args.cargo_home.is_symlink():
        parser.error("--cargo-home must be a real private directory below repository scratch")
    seed = args.seed_cargo_home or Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    STATE.mkdir(parents=True, exist_ok=True)
    with (STATE / "run.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.error("another focused check owns the stable checkout; retry after it finishes")
        cache.mkdir(parents=True, exist_ok=True, mode=0o700)
        if any((cache / name).exists() for name in ("config", "config.toml", "credentials", "credentials.toml")):
            parser.error("private Cargo cache must not contain settings or credentials")
        for name in ("registry", "git"):
            destination = cache / name
            if destination.is_symlink():
                parser.error("private dependency directories must be real directories")
            if not destination.exists() and (seed / name).exists():
                source = seed / name
                if source.is_symlink():
                    parser.error("seed dependency directories must be real directories")
                if sys.platform != "darwin" or subprocess.run(["/bin/cp", "-cR", str(source), str(destination)],
                                                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
                    if destination.exists():
                        shutil.rmtree(destination)
                    shutil.copytree(source, destination)
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        output = STATE / "runs" / (stamp + "-" + str(os.getpid()))
        output.mkdir(parents=True, mode=0o700)
        spec = importlib.util.spec_from_file_location("verifier", ROOT / "scripts/verify.py")
        verifier = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(verifier)
        digest, files = inventory(verifier, ROOT)
        slot = STATE / "source"
        changed = sync_source(ROOT, slot, files)
        if inventory(verifier, slot) != (digest, files) or inventory(verifier, ROOT) != (digest, files):
            raise ValueError("source changed while freezing focused checks")
        report = {"schema": "ca.check-change.v1", "status": "running", "full_gate": False,
                  "source_digest": digest, "source_files": files, "source": str(slot),
                  "cargo_home": str(cache), "changed_files": changed, "stages": [],
                  "prepare_seconds": round(time.monotonic() - prepared_at, 3)}
        env = environment(output, cache)
        report["isolation"] = {"home": env["HOME"], "tmp": env["TMPDIR"], "credentials_inherited": False}
        if args.prepare_only:
            report["status"] = "prepared"
            verifier.write_json(output / "report.json", report)
            print("[check-change] prepared: " + str(output / "report.json"), flush=True)
            return 0
        verifier.ROOT = slot
        features = ["--all-features"] if args.all_features else []
        commands = [("focused", ["cargo", "test", "--locked", "--offline", "--bin", "ca", *features,
                                 "--", *args.selector, "--test-threads=1"]),
                    ("format", ["cargo", "fmt", "--all", "--", "--check"]),
                    ("lint", ["cargo", "clippy", "--locked", "--offline", "--bin", "ca", *features, "--", "-D", "warnings"])]
        if args.release:
            commands.append(("release", ["cargo", "build", "--release", "--bin", "ca", "--all-features"]))
        started = time.monotonic()
        for name, command in commands:
            stage = verifier.run_stage(name, command, output, env, 3600, "test" if name == "focused" else "check")
            if name == "focused":
                missing = unmatched_selectors(Path(stage["log"]).read_text(), args.selector)
                if missing:
                    stage.update(status="failed", missing_selectors=missing)
            report["stages"].append(stage)
            verifier.write_json(output / "report.json", report)
            if stage["status"] != "passed":
                break
        report["source_stable"] = inventory(verifier, ROOT) == (digest, files) == inventory(verifier, slot)
        report["status"] = "passed" if report["source_stable"] and len(report["stages"]) == len(commands) and all(s["status"] == "passed" for s in report["stages"]) else "failed"
        if args.release and report["status"] == "passed":
            binary = ROOT / "target/release/ca"
            report["release"] = {"binary": str(binary), "sha256": sha(binary), "source_digest": digest}
        report["duration_seconds"] = round(time.monotonic() - started, 3)
        verifier.write_json(output / "report.json", report)
        print("[check-change] " + report["status"] + ": " + str(output / "report.json"), flush=True)
        return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
