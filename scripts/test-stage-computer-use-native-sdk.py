#!/usr/bin/env python3
"""Staging contracts use inert fixtures; real SDK/loader acceptance is separate."""
import importlib.util
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("native_sdk_stage", HERE / "stage-computer-use-native-sdk.py")
stage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage)


class NativeSdkStage(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.runtime = self.root / "runtime"
        self.destination = self.root / "resources"
        self.runtime.mkdir(mode=0o700)
        for relative in stage.audit.REQUIRED:
            path = self.runtime / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"\x7fELF-inert-fixture" if relative in stage.audit.LIBRARIES else b"inert notice")
        (self.runtime / "share/licenses/pipewire/SOURCE.txt").write_text("\n".join(stage.audit.SOURCE_LINES) + "\n")
        hashes = {p: stage.audit.digest(self.runtime / p) for p in stage.audit.NOTICE_HASHES}
        self.addCleanup(patch.stopall)
        patch.object(stage.audit, "NOTICE_HASHES", hashes).start()
        patch.object(stage.audit, "inspect_elf", return_value={"scope": "inert contract"}).start()

    def test_audited_copy_has_sanitized_manifest_and_reuses_identical_tree(self):
        self.assertIn("staged", stage.stage(self.runtime, self.destination))
        index = self.destination / "manifest.json"
        before = index.read_bytes()
        value = json.loads(before)
        self.assertEqual(set(value), {"schemaVersion", "target", "pipewireVersion", "glibcBaseline", "files"})
        self.assertNotIn(str(self.root), before.decode())
        self.assertEqual(len(value["files"]), 16)
        self.assertIn("reused", stage.stage(self.runtime, self.destination))
        self.assertEqual(index.read_bytes(), before)

    @unittest.skipUnless(sys.platform == "linux", "POSIX package permission contract")
    def test_root_owned_package_copy_readable_by_ordinary_user_without_source_changes(self):
        source_modes = {path: stat.S_IMODE(path.stat().st_mode) for path in (self.runtime, *self.runtime.rglob("*"))}
        previous = os.umask(0o077)
        try:
            stage.stage(self.runtime, self.destination)
        finally:
            os.umask(previous)
        for path in (self.destination, *self.destination.rglob("*")):
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o755 if path.is_dir() else 0o644)
        self.assertEqual(source_modes, {p: stat.S_IMODE(p.stat().st_mode) for p in source_modes})

    def test_relative_output_rejected_before_creation(self):
        with self.assertRaisesRegex(stage.audit.AuditError, "Absolute"):
            stage.stage(self.runtime, Path("relative-output"))

    def test_existing_unrelated_tree_is_never_overwritten(self):
        self.destination.mkdir()
        marker = self.destination / "keep"
        marker.write_text("keep original")
        with self.assertRaises(stage.audit.AuditError):
            stage.stage(self.runtime, self.destination)
        self.assertEqual(marker.read_text(), "keep original")

    def test_manifest_and_component_changes_rejected_without_repair(self):
        stage.stage(self.runtime, self.destination)
        index = self.destination / "manifest.json"
        original = index.read_bytes()
        index.write_text("{}")
        with self.assertRaisesRegex(stage.audit.AuditError, "refusing overwrite"):
            stage.stage(self.runtime, self.destination)
        self.assertEqual(index.read_text(), "{}")
        index.write_bytes(original)
        library = self.destination / "runtime" / stage.audit.LIBRARIES[0]
        library.write_bytes(b"\x7fELF-modified-fixture")
        with self.assertRaisesRegex(stage.audit.AuditError, "refusing overwrite"):
            stage.stage(self.runtime, self.destination)
        self.assertEqual(library.read_bytes(), b"\x7fELF-modified-fixture")

    @unittest.skipUnless(sys.platform == "linux", "native link/FIFO semantics")
    def test_linked_parent_and_special_source_refused_without_creation(self):
        alias = self.root / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(stage.audit.AuditError):
            stage.stage(self.runtime, alias / "not-created")
        self.assertFalse((self.root / "not-created").exists())
        os.mkfifo(self.runtime / "fifo")
        with self.assertRaises(stage.audit.AuditError):
            stage.stage(self.runtime, self.destination)
        self.assertFalse(self.destination.exists())

    @unittest.skipUnless(sys.platform == "linux", "POSIX mode semantics")
    def test_existing_private_permissions_are_not_silently_repaired(self):
        stage.stage(self.runtime, self.destination)
        self.destination.chmod(0o700)
        with self.assertRaisesRegex(stage.audit.AuditError, "permissions differ"):
            stage.stage(self.runtime, self.destination)
        self.assertEqual(stat.S_IMODE(self.destination.stat().st_mode), 0o700)

    def test_source_changed_during_copy_never_publishes_manifest(self):
        original = stage.shutil.copytree
        def changed(*args, **kwargs):
            result = original(*args, **kwargs)
            if Path(args[1]) == self.destination / "runtime":
                (self.destination / "runtime/lib/unapproved.so").write_bytes(b"unexpected")
            return result
        with patch.object(stage.shutil, "copytree", side_effect=changed):
            with self.assertRaises(stage.audit.AuditError):
                stage.stage(self.runtime, self.destination)
        self.assertFalse((self.destination / "manifest.json").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
