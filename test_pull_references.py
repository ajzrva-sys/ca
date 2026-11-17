#!/usr/bin/env python3
"""Offline integration tests; all Git fixtures live in the system scratch directory."""
import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name("pull-references.py")
NAMES = ("hermes-agent", "grok-build", "mimocode", "codex")
SCRATCH = Path("/Users/aji/.hermes/cache/scratch")


class RefreshTests(unittest.TestCase):
    def load_script(self):
        self.assertTrue(SCRIPT.is_file(), "The reference refresh script has not been written")
        spec = importlib.util.spec_from_file_location("pull_references", SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def git(self, source, *args):
        result = subprocess.run(
            ["git", "-C", str(source), *args],
            check=True, capture_output=True, text=True,
        )
        return result.stdout.strip()

    def make_source(self, root):
        source = root / "upstream"
        source.mkdir()
        self.git(source, "init", "--quiet", "--initial-branch=reference-default")
        (source / "README.md").write_text("first revision\n")
        self.git(source, "add", "README.md")
        self.git(source, "-c", "user.name=Reference Test", "-c",
                 "user.email=reference-test@example.invalid", "commit", "--quiet", "-m", "first")
        return source

    def test_refresh_replaces_stale_content_and_tracks_latest_default_branch(self):
        module = self.load_script()
        with tempfile.TemporaryDirectory(prefix="reference-tests-", dir=SCRATCH) as temp:
            root = Path(temp)
            source = self.make_source(root)
            destination = root / "references with spaces"
            destination.mkdir()
            (destination / "stale.txt").write_text("discard this")
            destination_inode = destination.stat().st_ino
            repositories = {name: source.as_uri() for name in NAMES}
            module.refresh_references(destination, repositories)
            self.assertEqual(destination.stat().st_ino, destination_inode)
            self.assertFalse((destination / "stale.txt").exists())
            manifest = json.loads((destination / "REFERENCES.json").read_text())
            self.assertEqual({row["name"] for row in manifest["repositories"]}, set(NAMES))
            for row in manifest["repositories"]:
                checkout = destination / row["name"]
                self.assertEqual(row["commit"], self.git(source, "rev-parse", "HEAD"))
                self.assertEqual(row["branch"], "reference-default")
                self.assertEqual(self.git(checkout, "rev-parse", "--is-shallow-repository"), "true")
                self.assertEqual(self.git(checkout, "status", "--porcelain"), "")
            (source / "README.md").write_text("second revision\n")
            self.git(source, "add", "README.md")
            self.git(source, "-c", "user.name=Reference Test", "-c",
                     "user.email=reference-test@example.invalid", "commit", "--quiet", "-m", "second")
            (destination / "codex" / "README.md").write_text("local edit to discard\n")
            module.refresh_references(destination, repositories)
            self.assertEqual((destination / "codex" / "README.md").read_text(), "second revision\n")
            manifest = json.loads((destination / "REFERENCES.json").read_text())
            self.assertTrue(all(row["commit"] == self.git(source, "rev-parse", "HEAD")
                                for row in manifest["repositories"]))

    def test_publish_failure_restores_previous_contents(self):
        module = self.load_script()
        with tempfile.TemporaryDirectory(prefix="reference-tests-", dir=SCRATCH) as temp:
            root = Path(temp)
            source = self.make_source(root)
            destination = root / "references"
            destination.mkdir()
            (destination / "old-reference").mkdir()
            (destination / "old-reference" / "README.md").write_text("keep old source")
            (destination / ".hidden").write_text("keep hidden entry")
            (destination / "external-link").symlink_to(source, target_is_directory=True)
            original_rename = Path.rename

            def fail_publish_once(path, target):
                if path.name == "codex" and Path(target).parent == destination:
                    raise OSError("intentional publication failure")
                return original_rename(path, target)

            with mock.patch.object(Path, "rename", fail_publish_once):
                with self.assertRaisesRegex(OSError, "intentional publication failure"):
                    module.refresh_references(destination, {name: source.as_uri() for name in NAMES})
            self.assertEqual({entry.name for entry in destination.iterdir()},
                             {"old-reference", ".hidden", "external-link"})
            self.assertEqual((destination / "old-reference" / "README.md").read_text(), "keep old source")
            self.assertEqual((destination / ".hidden").read_text(), "keep hidden entry")
            self.assertTrue((destination / "external-link").is_symlink())
            self.assertEqual((source / "README.md").read_text(), "first revision\n")

    def test_dry_run_defaults_to_current_directory_without_writing(self):
        with tempfile.TemporaryDirectory(prefix="reference-tests-", dir=SCRATCH) as temp:
            destination = Path(temp)
            (destination / "keep.txt").write_text("untouched")
            result = subprocess.run(["python3", str(SCRIPT), "--dry-run"], cwd=destination,
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(str(destination), result.stdout)
            for name in NAMES:
                self.assertIn(name, result.stdout)
            self.assertEqual({entry.name for entry in destination.iterdir()}, {"keep.txt"})

    def test_validation_rejects_wrong_destinations_before_fetching(self):
        module = self.load_script()
        self.assertTrue(hasattr(module, "validate_destination"), "Destination validation is missing")
        with tempfile.TemporaryDirectory(prefix="reference-tests-", dir=SCRATCH) as temp:
            root = Path(temp)
            repository = root / "ca"
            repository.mkdir()
            (repository / ".git").mkdir()
            references = repository / "oracles"
            references.mkdir()
            link = root / "linked-references"
            link.symlink_to(references, target_is_directory=True)
            invalid = [Path("/"), Path.home(), SCRIPT.parent, repository, link]
            for destination in invalid:
                with self.subTest(destination=destination):
                    with self.assertRaises(ValueError):
                        module.validate_destination(destination)
            self.assertEqual(module.validate_destination(references), references)

    def test_failed_fetch_leaves_existing_content_untouched(self):
        module = self.load_script()
        with tempfile.TemporaryDirectory(prefix="reference-tests-", dir=SCRATCH) as temp:
            root = Path(temp)
            source = self.make_source(root)
            destination = root / "references"
            destination.mkdir()
            (destination / "keep.txt").write_text("untouched")
            repositories = {name: source.as_uri() for name in NAMES}
            repositories["codex"] = (root / "missing-upstream").as_uri()
            with self.assertRaises(subprocess.CalledProcessError):
                module.refresh_references(destination, repositories)
            self.assertEqual({entry.name for entry in destination.iterdir()}, {"keep.txt"})
            self.assertEqual((destination / "keep.txt").read_text(), "untouched")
            self.assertFalse(list(root.glob(".references-refresh-*")))


if __name__ == "__main__":
    unittest.main()
