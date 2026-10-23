"""Real private filesystem transactions; executable probes are fixture doubles."""
import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release_install_fixture", Path(__file__).with_name("automation_install.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class MatchedInstallTransactions(unittest.TestCase):
    def exercise(self, count, fail_last=False):
        names = installer.TRIO_NAMES[:count]
        with tempfile.TemporaryDirectory(dir=Path(tempfile.gettempdir()).resolve()) as temporary:
            root = Path(temporary)
            destination_root, candidate_root = root / "bin", root / "candidate"
            destination_root.mkdir(mode=0o700)
            candidate_root.mkdir(mode=0o700)
            for name in names:
                for directory, content in ((destination_root, b"before "), (candidate_root, b"after ")):
                    binary = directory / name
                    binary.write_bytes(content + name.encode())
                    binary.chmod(0o755)

            def inspect(path, name, new_release=False):
                return {"file": installer.fingerprint(path), "version": name + " 0.1.0",
                        "semver": "0.1.0", "help_sha256": "help", "interfaces": {},
                        "signature": "fixture", "code_directory": None}

            def signature(path, sign=False):
                if fail_last and sign and Path(path) == destination_root / names[-1]:
                    raise installer.InstallError("fixture installed signer failure")

            with patch.object(installer, "NAMES", names), patch.object(installer, "inspect", inspect), \
                    patch.object(installer, "signature", signature), \
                    patch.object(installer.shutil, "which", lambda name: str(destination_root / name)), \
                    contextlib.redirect_stdout(io.StringIO()):
                entries = [{"name": name, "candidate": str(candidate_root / name),
                            "destination": str(destination_root / name),
                            "destination_parent": installer.parent_identity(destination_root / name),
                            "before": inspect(destination_root / name, name),
                            "after": inspect(candidate_root / name, name, True)} for name in names]
                if fail_last:
                    with self.assertRaisesRegex(installer.InstallError, "signer failure"):
                        installer.install(entries)
                else:
                    receipt = installer.install(entries)
                    self.assertEqual(receipt["phase"], "completed")
                    for name in names:
                        self.assertEqual((destination_root / name).read_bytes(), b"after " + name.encode())
                    receipt_path = Path(entries[0]["stage_directory"]) / "paired-install.json"
                    # Recovery discovers the trio even with legacy pair defaults.
                    installer.NAMES = ("ca", "codex-team")
                    recovered = installer.recover(receipt_path)
                    self.assertEqual(recovered["phase"], "rolled_back")
                for name in names:
                    self.assertEqual((destination_root / name).read_bytes(), b"before " + name.encode())
                    self.assertEqual((destination_root / name).stat().st_mode & 0o777, 0o755)

    def test_pair_install_and_recovery_remain_compatible(self):
        self.exercise(2)

    def test_trio_install_and_recovery_restore_all_members(self):
        self.exercise(3)

    def test_last_member_signer_failure_rolls_back_entire_trio(self):
        self.exercise(3, fail_last=True)
