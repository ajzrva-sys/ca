import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('results', Path(__file__).with_name('results.py'))
results = importlib.util.module_from_spec(spec)
spec.loader.exec_module(results)


class Results(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        self.task = self.root / 'task'
        self.task.mkdir()

    def tearDown(self):
        self.temporary.cleanup()

    def test_exact_binary_empty_and_nested_bytes(self):
        (self.task / 'src').mkdir()
        (self.task / 'src/data').write_bytes(b'\x00\xffhello')
        (self.task / 'empty').write_bytes(b'')
        self.assertEqual(dict(results.snapshot(self.task)), {'src/data': [0, 255, 104, 101, 108, 108, 111], 'empty': []})

    def test_root_and_directory_links_are_rejected(self):
        link = self.root / 'alias'
        link.symlink_to(self.task)
        with self.assertRaises(OSError):
            results.snapshot(link)
        (self.task / 'alias').symlink_to(self.root)
        with self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_file_link_does_not_read_outside(self):
        (self.root / 'secret').write_text('private')
        (self.task / 'alias').symlink_to(self.root / 'secret')
        with self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_hard_link_is_rejected(self):
        (self.root / 'secret').write_text('private')
        os.link(self.root / 'secret', self.task / 'alias')
        with self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_fifo_is_rejected_without_blocking(self):
        os.mkfifo(self.task / 'fifo')
        with self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_total_byte_limit(self):
        (self.task / 'a').write_bytes(b'123')
        (self.task / 'b').write_bytes(b'456')
        with patch.object(results, 'MAX_BYTES', 5), self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_directory_count_limit(self):
        for name in ('a', 'b', 'c'):
            (self.task / name).mkdir()
        with patch.object(results, 'MAX_ENTRIES', 2), self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_depth_limit(self):
        (self.task / 'a/b/c').mkdir(parents=True)
        with patch.object(results, 'MAX_DEPTH', 2), self.assertRaises(ValueError):
            results.snapshot(self.task)

    def test_changed_descriptor_is_rejected(self):
        (self.task / 'data').write_bytes(b'initial')
        original = results.os.fstat
        seen = 0
        def changed(fd):
            nonlocal seen
            metadata = original(fd)
            if os.path.isfile(self.task / 'data') and metadata.st_ino == (self.task / 'data').stat().st_ino:
                seen += 1
                if seen == 2:
                    (self.task / 'data').write_bytes(b'changed bytes')
                    return original(fd)
            return metadata
        with patch.object(results.os, 'fstat', side_effect=changed), self.assertRaises(ValueError):
            results.snapshot(self.task)


if __name__ == '__main__':
    unittest.main()
