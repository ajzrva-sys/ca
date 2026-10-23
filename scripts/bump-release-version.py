#!/usr/bin/env python3
"""Advance the root ca package patch once in Cargo.toml and Cargo.lock.

Workspace member crates stay at their current versions. A pre-release or any
version that is not MAJOR.MINOR.PATCH is refused, and nothing is written.
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class BumpError(Exception):
    """The manifests cannot be advanced safely."""


def quoted_assignment(stripped: str, key: str) -> str:
    if not stripped.startswith(key):
        raise BumpError(f"expected {key}")
    rest = stripped[len(key):].strip()
    if not rest.startswith("="):
        raise BumpError(f"unrecognized {key} line: {stripped}")
    rest = rest[1:].strip()
    if len(rest) < 2 or rest[0] != '"' or rest[-1] != '"':
        raise BumpError(f"unrecognized {key} line: {stripped}")
    return rest[1:-1]


def plain_version(value: str) -> tuple[str, str, str]:
    parts = value.split(".")
    if len(parts) != 3 or not all(part.isdigit() and part != "" for part in parts):
        raise BumpError(f"refusing pre-release or non-patch version {value}")
    return parts[0], parts[1], parts[2]


def next_patch(value: str) -> str:
    major, minor, patch = plain_version(value)
    return f"{major}.{minor}.{int(patch) + 1}"


def version_line(original: str, version: str) -> str:
    newline = ""
    body = original
    if body.endswith("\r\n"):
        newline = "\r\n"
        body = body[:-2]
    elif body.endswith("\n"):
        newline = "\n"
        body = body[:-1]
    indent = body[:len(body) - len(body.lstrip(" \t"))]
    return f'{indent}version = "{version}"{newline}'


def package_version(text: str) -> tuple[int, str]:
    lines = text.splitlines()
    in_package = False
    found: tuple[int, str] | None = None
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("[") and stripped.endswith("]"):
            in_package = False
            continue
        if not in_package or not stripped.startswith("version"):
            continue
        version = quoted_assignment(stripped, "version")
        plain_version(version)
        if found is not None:
            raise BumpError("multiple versions in [package]")
        found = (index, version)
    if found is None:
        raise BumpError("Cargo.toml has no [package] version")
    return found


def bump_lock(text: str, old: str, new: str) -> str:
    lines = text.splitlines(keepends=True)
    starts = [index for index, line in enumerate(lines) if line.strip() == "[[package]]"]
    bounds = starts + [len(lines)]
    updated = 0
    for start, end in zip(bounds, bounds[1:]):
        name = None
        version_at = None
        for index in range(start + 1, end):
            stripped = lines[index].strip()
            if name is None and stripped.startswith("name"):
                name = quoted_assignment(stripped, "name")
                continue
            if name is not None and stripped.startswith("version"):
                version_at = index
                break
        if name != "ca":
            continue
        if version_at is None:
            raise BumpError("lockfile ca package has no version")
        current = quoted_assignment(lines[version_at].strip(), "version")
        plain_version(current)
        if current != old:
            raise BumpError(
                f"lockfile ca version {current} does not match Cargo.toml {old}"
            )
        lines[version_at] = version_line(lines[version_at], new)
        updated += 1
    if updated != 1:
        raise BumpError(f"expected one ca package in the lockfile, found {updated}")
    return "".join(lines)


def write_atomic(path: Path, text: str) -> None:
    temporary = path.with_name(path.name + ".bump-tmp")
    temporary.write_text(text)
    temporary.replace(path)


def bump_files(toml_path: Path, lock_path: Path) -> tuple[str, str]:
    toml_text = toml_path.read_text()
    lock_text = lock_path.read_text()
    index, old = package_version(toml_text)
    new = next_patch(old)
    toml_lines = toml_text.splitlines(keepends=True)
    toml_lines[index] = version_line(toml_lines[index], new)
    new_toml = "".join(toml_lines)
    new_lock = bump_lock(lock_text, old, new)
    write_atomic(toml_path, new_toml)
    write_atomic(lock_path, new_lock)
    return old, new


def self_test() -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        toml_path = root / "Cargo.toml"
        lock_path = root / "Cargo.lock"
        toml_path.write_text(
            "\n".join([
                "[workspace]",
                'members = ["."]',
                "",
                "[package]",
                'name = "ca"',
                'version = "0.1.0"',
                'edition = "2021"',
                "",
                "[dependencies]",
                'byteorder-lite = { version = "0.1.0" }',
                'pinned = "0.1.0"',
                "",
            ])
        )
        lock_path.write_text(
            "\n".join([
                "version = 4",
                "",
                "[[package]]",
                'name = "byteorder-lite"',
                'version = "0.1.0"',
                "",
                "[[package]]",
                'name = "ca"',
                'version = "0.1.0"',
                "dependencies = [",
                ' "byteorder-lite",',
                "]",
                "",
                "[[package]]",
                'name = "ca-watch"',
                'version = "0.1.0"',
                "",
            ])
        )
        old, new = bump_files(toml_path, lock_path)
        if (old, new) != ("0.1.0", "0.1.1"):
            raise BumpError(f"expected 0.1.0 -> 0.1.1, got {old} -> {new}")
        toml_text = toml_path.read_text()
        if 'name = "ca"\nversion = "0.1.1"' not in toml_text:
            raise BumpError("package version was not advanced")
        if '{ version = "0.1.0" }' not in toml_text or 'pinned = "0.1.0"' not in toml_text:
            raise BumpError("dependency versions changed")
        lock_text = lock_path.read_text()
        if 'name = "ca"\nversion = "0.1.1"' not in lock_text:
            raise BumpError("lockfile ca version was not advanced")
        if 'name = "byteorder-lite"\nversion = "0.1.0"' not in lock_text:
            raise BumpError("unrelated lock package changed")
        if 'name = "ca-watch"\nversion = "0.1.0"' not in lock_text:
            raise BumpError("workspace member lock version changed")

        toml_path.write_text('[package]\nname = "ca"\nversion = "1.2.9"\n')
        lock_path.write_text('[[package]]\nname = "ca"\nversion = "1.2.9"\n')
        old, new = bump_files(toml_path, lock_path)
        if (old, new) != ("1.2.9", "1.2.10"):
            raise BumpError(f"expected 1.2.9 -> 1.2.10, got {old} -> {new}")

        original = '[package]\nname = "ca"\nversion = "0.1.0-alpha.1"\n'
        toml_path.write_text(original)
        lock_before = lock_path.read_text()
        try:
            bump_files(toml_path, lock_path)
        except BumpError:
            pass
        else:
            raise BumpError("pre-release version was accepted")
        if toml_path.read_text() != original or lock_path.read_text() != lock_before:
            raise BumpError("pre-release refusal modified a manifest")
    print("bump-release-version self-test passed")


def main(argv: list[str]) -> None:
    if argv == ["--self-test"]:
        self_test()
        return
    if not argv:
        toml_path = ROOT / "Cargo.toml"
        lock_path = ROOT / "Cargo.lock"
    elif len(argv) == 2:
        toml_path = Path(argv[0])
        lock_path = Path(argv[1])
    else:
        raise SystemExit(
            "usage: bump-release-version.py [--self-test] [Cargo.toml Cargo.lock]"
        )
    try:
        old, new = bump_files(toml_path, lock_path)
    except BumpError as error:
        raise SystemExit(str(error)) from error
    print(f"{old} -> {new}")


if __name__ == "__main__":
    main(sys.argv[1:])
