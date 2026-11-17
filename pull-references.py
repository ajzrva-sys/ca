#!/usr/bin/env python3
"""Refresh CA's source references, replacing the chosen directory's contents.

Usage:
    /Volumes/projects/ca/pull-references.py
    /Volumes/projects/ca/pull-references.py /Volumes/projects/ca/ca/oracles

The default destination is the current working directory. "Latest" means the
latest commit on each upstream repository's default branch, not a release tag.
Requires Python 3 and Git. No builds, installations, or submodule execution.
"""
import argparse
import concurrent.futures
import datetime
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPOSITORIES = {
    "hermes-agent": "https://github.com/NousResearch/hermes-agent.git",
    "grok-build": "https://github.com/xai-org/grok-build.git",
    "mimocode": "https://github.com/XiaomiMiMo/MiMo-Code.git",
    "codex": "https://github.com/openai/codex.git",
}


def validate_destination(destination):
    """Catch a mistaken source-repository, script-directory, or symlink target."""
    requested = Path(destination).expanduser()
    if requested.is_symlink():
        raise ValueError("Choose the real reference directory, not a symlink: {}".format(requested))
    destination = requested.resolve()
    script = Path(__file__).resolve()
    if destination == Path(destination.anchor) or destination == Path.home().resolve():
        raise ValueError("Choose a reference directory, not the root or home directory")
    if destination == script.parent or destination in script.parents:
        raise ValueError("Destination contains this script; choose /Volumes/projects/ca/ca/oracles instead")
    if (destination / ".git").exists():
        raise ValueError("Destination is a source Git repository; choose its reference subdirectory instead")
    if destination.exists() and not destination.is_dir():
        raise ValueError("Destination is not a directory: {}".format(destination))
    return destination


def git(*args):
    """Run Git without an interactive credentials prompt."""
    environment = dict(os.environ, GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="never")
    result = subprocess.run(
        ["git", *map(str, args)], check=True, capture_output=True, text=True,
        env=environment, timeout=600,
    )
    return result.stdout.strip()


def fetch_reference(name, repository, stage):
    """Fetch and identify one shallow default-branch checkout."""
    checkout = stage / name
    print("Fetching {} ...".format(name), flush=True)
    git("clone", "--quiet", "--depth=1", "--single-branch", "--no-tags",
        "--", repository, checkout)
    return {
        "name": name,
        "repository": repository,
        "branch": git("-C", checkout, "symbolic-ref", "--short", "HEAD"),
        "commit": git("-C", checkout, "rev-parse", "HEAD"),
        "commit_date": git("-C", checkout, "show", "-s", "--format=%cI", "HEAD"),
    }


def refresh_references(destination, repositories=None):
    """Fetch every reference before replacing any existing reference."""
    destination = validate_destination(destination)
    repositories = REPOSITORIES if repositories is None else repositories
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Stage on the destination filesystem so publishing does not copy huge trees.
    temporary = Path(tempfile.mkdtemp(
        prefix=".{}-refresh-".format(destination.name), dir=destination.parent,
    ))
    preserve_recovery = False
    try:
        stage = temporary / "new"
        stage.mkdir()
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
            futures = [executor.submit(fetch_reference, name, url, stage)
                       for name, url in repositories.items()]
            records = [future.result() for future in futures]
        manifest = {
            "fetched_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "selection": "upstream default-branch HEAD; shallow source checkouts",
            "repositories": records,
        }
        (stage / "REFERENCES.json").write_text(json.dumps(manifest, indent=2) + "\n")
        destination.mkdir(exist_ok=True)
        backup = temporary / "previous"
        backup.mkdir()
        published = []
        try:
            for entry in list(destination.iterdir()):
                entry.rename(backup / entry.name)
            for entry in list(stage.iterdir()):
                target = destination / entry.name
                entry.rename(target)
                published.append(target)
        except BaseException:
            try:
                for entry in published:
                    entry.rename(stage / entry.name)
                for entry in list(backup.iterdir()):
                    entry.rename(destination / entry.name)
            except BaseException:
                preserve_recovery = True
                print("Rollback failed; recovery files are in {}".format(temporary), file=sys.stderr)
                raise
            raise
    finally:
        if not preserve_recovery:
            shutil.rmtree(temporary)
    print("Refreshed {} references in {}".format(len(records), destination), flush=True)
    for record in records:
        print("  {}  {}  {}".format(record["name"], record["branch"], record["commit"]))
    return manifest


def main():
    parser = argparse.ArgumentParser(
        description="Replace a reference directory with the latest Hermes, Grok Build, MiMo-Code and Codex sources.",
        epilog="All existing destination contents are replaced only after all four fetches succeed.",
    )
    parser.add_argument("destination", nargs="?", default=Path.cwd(), type=Path,
                        help="reference directory to replace (default: current directory)")
    parser.add_argument("--dry-run", action="store_true",
                        help="show destination and sources without fetching or changing files")
    args = parser.parse_args()
    try:
        destination = validate_destination(args.destination)
        print("Destination: {} (existing contents will be replaced)".format(destination), flush=True)
        if args.dry_run:
            for name, repository in REPOSITORIES.items():
                print("  {} <- {} (default-branch HEAD)".format(name, repository))
            return 0
        refresh_references(destination)
    except subprocess.CalledProcessError as error:
        print("Git failed: {}".format(error.stderr.strip()), file=sys.stderr)
        return 1
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print("Refresh failed: {}".format(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
