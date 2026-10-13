#!/usr/bin/env python3
"""Private compositor source/ownership guards, not native acceptance."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().with_name("build-computer-use-labwc.sh")


@unittest.skipUnless(sys.platform == "linux", "Linux compositor build guards")
class DestinationGuards(unittest.TestCase):
    def run_script(self, *args, env=None):
        return subprocess.run(["bash", str(SCRIPT), *map(str, args)], env=env,
                              text=True, capture_output=True, timeout=15)

    def test_root_and_relative_paths_are_rejected(self):
        for path in ("/", "relative"):
            with self.subTest(path=path):
                self.assertEqual(self.run_script(path).returncode, 2)

    def test_existing_destination_remains_untouched(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            sentinel = root / "sentinel"
            sentinel.write_text("keep existing source")
            self.assertEqual(self.run_script(root).returncode, 2)
            self.assertEqual(list(root.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_text(), "keep existing source")

    def test_symlink_destination_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            link = Path(temp) / "linked"
            link.symlink_to("missing", target_is_directory=True)
            self.assertEqual(self.run_script(link).returncode, 2)
            self.assertFalse((Path(temp) / "missing").exists())

    def test_symlink_parent_is_rejected_before_creation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "real").mkdir()
            (root / "link").symlink_to(root / "real", target_is_directory=True)
            result = self.run_script(root / "link" / "new")
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((root / "real" / "new").exists())

    def test_parent_traversal_is_rejected_before_creation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "child").mkdir()
            result = self.run_script(root / "child" / ".." / "new")
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertFalse((root / "new").exists())

    def test_wrong_source_bytes_are_rejected_before_extraction(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "wrong.tar.gz"
            archive.write_bytes(b"incorrect pinned source")
            destination = root / "new"
            env = dict(os.environ, LABWC_SOURCE_ARCHIVE=str(archive))
            result = self.run_script(destination, env=env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("FAILED", result.stdout)
            self.assertFalse((destination / "source").exists())
            self.assertFalse((destination / "bin").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
