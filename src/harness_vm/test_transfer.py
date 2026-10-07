import hashlib
import importlib.util
import io
import os
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('transfer', Path(__file__).with_name('transfer.py'))
transfer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transfer)


def archive(entries):
    result = io.BytesIO()
    with tarfile.open(fileobj=result, mode='w', format=tarfile.GNU_FORMAT) as package:
        for name, data, kind in entries:
            header = tarfile.TarInfo(name)
            header.size = len(data) if kind == tarfile.REGTYPE else 0
            header.mode = 0o7777
            header.type = kind
            header.linkname = '/etc/passwd' if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE) else ''
            package.addfile(header, io.BytesIO(data) if header.size else None)
    result.seek(0)
    return result


class Transfer(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.saved = transfer.MAX_FILE, transfer.MAX_TOTAL, transfer.MAX_ENTRIES

    def tearDown(self):
        transfer.MAX_FILE, transfer.MAX_TOTAL, transfer.MAX_ENTRIES = self.saved
        self.temporary.cleanup()

    def reject(self, entries):
        with self.assertRaises((ValueError, FileExistsError, tarfile.TarError)):
            transfer.extract(archive(entries), self.root / 'out')
        self.assertFalse((self.root / 'escape').exists())

    def test_regular_files_have_normal_modes_and_no_special_bits(self):
        old = os.umask(0o077)
        try:
            count, size = transfer.extract(archive([('src/main.rs', b'hello', tarfile.REGTYPE)]), self.root / 'out')
        finally:
            os.umask(old)
        self.assertEqual((count, size), (1, 5))
        self.assertEqual((self.root / 'out/src/main.rs').read_bytes(), b'hello')
        self.assertEqual((self.root / 'out/src/main.rs').stat().st_mode & 0o7777, 0o755)
        self.assertEqual((self.root / 'out/src').stat().st_mode & 0o7777, 0o755)

    def test_parent_traversal_rejected(self):
        self.reject([('../escape', b'x', tarfile.REGTYPE)])

    def test_absolute_paths_rejected(self):
        self.reject([('/escape', b'x', tarfile.REGTYPE)])

    def test_symlinks_rejected(self):
        self.reject([('link', b'', tarfile.SYMTYPE)])

    def test_hardlinks_rejected(self):
        self.reject([('link', b'', tarfile.LNKTYPE)])

    def test_devices_rejected(self):
        self.reject([('device', b'', tarfile.CHRTYPE)])

    def test_duplicate_paths_rejected(self):
        self.reject([('same', b'x', tarfile.REGTYPE), ('same', b'y', tarfile.REGTYPE)])

    def test_parent_file_collision_rejected(self):
        self.reject([('parent', b'x', tarfile.REGTYPE), ('parent/child', b'x', tarfile.REGTYPE)])

    def test_file_bound_rejected(self):
        transfer.MAX_FILE = 2
        self.reject([('large', b'123', tarfile.REGTYPE)])

    def test_total_bound_rejected(self):
        transfer.MAX_TOTAL = 3
        self.reject([('one', b'12', tarfile.REGTYPE), ('two', b'34', tarfile.REGTYPE)])

    def test_entry_bound_rejected(self):
        transfer.MAX_ENTRIES = 1
        self.reject([('one', b'1', tarfile.REGTYPE), ('two', b'2', tarfile.REGTYPE)])

    def test_build_outputs_are_not_inputs(self):
        self.reject([('target/release/ca', b'bad', tarfile.REGTYPE)])

    def test_existing_destination_is_never_overwritten(self):
        (self.root / 'out').mkdir()
        self.reject([('one', b'1', tarfile.REGTYPE)])

    def test_transfer_stream_checks_exact_bytes_and_limit(self):
        digest = hashlib.sha256(b'abc').hexdigest()
        with transfer.receive_stream(io.BytesIO(b'abc'), 3, digest) as result:
            self.assertEqual(result.read(), b'abc')
        with self.assertRaises(ValueError):
            transfer.receive_stream(io.BytesIO(b'abcd'), 3, digest)
        with self.assertRaises(ValueError):
            transfer.receive_stream(io.BytesIO(b'abd'), 3, digest)

    def test_export_rejects_parent_symlinks_and_hardlinks(self):
        (self.root / 'real').mkdir()
        (self.root / 'real/file').write_bytes(b'123')
        (self.root / 'alias').symlink_to(self.root / 'real', target_is_directory=True)
        with self.assertRaises(OSError):
            transfer.open_regular(str(self.root / 'alias/file'), 10)
        os.link(self.root / 'real/file', self.root / 'hard')
        with self.assertRaises(ValueError):
            transfer.open_regular(str(self.root / 'hard'), 10)

    def test_export_checks_file_kind_and_size(self):
        path = self.root / 'artifact'
        path.write_bytes(b'123')
        with self.assertRaises(ValueError):
            transfer.open_regular(str(path), 2)
        with self.assertRaises(ValueError):
            transfer.open_regular(str(self.root), 10)
        data, metadata = transfer.open_regular(str(path), 3)
        with data:
            self.assertEqual(data.read(), b'123')
            self.assertEqual(metadata.st_size, 3)


if __name__ == '__main__':
    unittest.main()
