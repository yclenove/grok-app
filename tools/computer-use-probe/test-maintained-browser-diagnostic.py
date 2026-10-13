"""Staging boundary oracles; synthetic binaries are not native acceptance."""
import hashlib
import importlib.util
import io
import stat
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import Mock, patch


SPEC = importlib.util.spec_from_file_location(
    "diagnostic", Path(__file__).with_name("stage-maintained-browser-diagnostic.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)


class DiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="grok-cu-diagnostic-oracle-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def zip(self, rows):
        path = self.root / "source.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name, data, mode in rows:
                entry = zipfile.ZipInfo(name)
                entry.external_attr = mode << 16
                archive.writestr(entry, data)
        return path

    def tar(self, rows):
        path = self.root / "source.tar.gz"
        with tarfile.open(path, "w:gz", format=tarfile.GNU_FORMAT) as archive:
            for name, data, kind in rows:
                entry = tarfile.TarInfo(name)
                entry.type, entry.size = kind, len(data)
                archive.addfile(entry, io.BytesIO(data))
        return path

    def test_windows_safe_path_contract_is_also_enforced_on_unix(self):
        for name in ("../outside", "a/../b", "a//b", "a\\b", "C:bad", "a\0b", "a/b.",
                     "a/b ", "a/CON.txt", "a/LPT1", "/absolute"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT.safe_parts(name)
        self.assertEqual(SUBJECT.safe_parts("a/locales/en-US.pak"), ["a", "locales", "en-US.pak"])

    def test_materialization_never_overwrites_and_truncation_is_not_success(self):
        path = self.root / "output" / "file"
        row = SUBJECT.copy_payload(io.BytesIO(b"data"), path, 4)
        self.assertEqual(row, {"bytes": 4, "sha256": hashlib.sha256(b"data").hexdigest()})
        with self.assertRaises(FileExistsError):
            SUBJECT.copy_payload(io.BytesIO(b"other"), path, 5)
        self.assertEqual(path.read_bytes(), b"data")
        for length, data in ((5, b"short"[:-1]), (1, b"too big")):
            with self.assertRaises(ValueError):
                SUBJECT.copy_payload(io.BytesIO(data), self.root / str(length), length)

    def test_new_output_free_space_and_parent_contract(self):
        (self.root / ".run").mkdir()
        with patch.object(SUBJECT, "HERE", self.root):
            with self.assertRaises(ValueError):
                SUBJECT.exclusive_output(self.root / "outside")
            with patch.object(SUBJECT.shutil, "disk_usage", return_value=Mock(free=0)), \
                    self.assertRaisesRegex(ValueError, "free"):
                SUBJECT.exclusive_output(self.root / ".run" / "new")
            output = SUBJECT.exclusive_output(self.root / ".run" / "new")
            with self.assertRaises(FileExistsError):
                SUBJECT.exclusive_output(output)

    def test_zip_selected_files_and_complete_tree_hash_are_actual_bytes(self):
        source = self.zip([("root/file", b"file", stat.S_IFREG | 0o644),
                           ("root/extra", b"extra", stat.S_IFREG | 0o644)])
        rows = SUBJECT.copy_zip(source, "root", self.root / "dest", {"file"})
        self.assertEqual(set(rows), {"file"})
        self.assertEqual(rows["file"]["sha256"], hashlib.sha256(b"file").hexdigest())
        self.assertFalse((self.root / "dest" / "extra").exists())

    def test_zip_traversal_case_duplicate_wrong_root_and_links_reject(self):
        for rows in ([('root/../outside', b"bad", stat.S_IFREG)],
                     [('wrong/file', b"bad", stat.S_IFREG)],
                     [('root/file', b"a", stat.S_IFREG), ('root/FILE', b"b", stat.S_IFREG)],
                     [('root/link', b"elsewhere", stat.S_IFLNK | 0o777)]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                SUBJECT.copy_zip(self.zip(rows), "root", self.root / "dest")

    def test_zip_directory_payload_and_missing_selected_file_reject(self):
        source = self.zip([("root/dir/", b"payload", stat.S_IFDIR | 0o755)])
        with self.assertRaises(ValueError):
            SUBJECT.copy_zip(source, "root", self.root / "dest")
        source = self.zip([("root/file", b"file", stat.S_IFREG | 0o644)])
        with self.assertRaisesRegex(ValueError, "missing"):
            SUBJECT.copy_zip(source, "root", self.root / "dest", {"missing"})

    def test_tar_node_only_selects_explicit_files_without_materializing_npm_links(self):
        source = self.tar([("root/bin/node", b"binary", tarfile.REGTYPE),
                           ("root/LICENSE", b"license", tarfile.REGTYPE),
                           ("root/npm", b"", tarfile.SYMTYPE)])
        rows = SUBJECT.copy_tar(source, "root", self.root / "dest", {"bin/node", "LICENSE"})
        self.assertEqual(set(rows), {"bin/node", "LICENSE"})
        self.assertFalse((self.root / "dest/npm").exists())

    def test_tar_links_special_objects_and_missing_selected_files_reject(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE):
            source = self.tar([("root/file", b"", kind)])
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                SUBJECT.copy_tar(source, "root", self.root / "dest")
        source = self.tar([("root/file", b"file", tarfile.REGTYPE)])
        with self.assertRaisesRegex(ValueError, "missing"):
            SUBJECT.copy_tar(source, "root", self.root / "dest", {"missing"})

    def test_bad_source_pin_precedes_all_output_writes(self):
        output = self.root / "new"
        with patch.dict(SUBJECT.NODE, {"inspect": Mock(side_effect=ValueError("bad source SHA"))}), \
                self.assertRaisesRegex(ValueError, "SHA"):
            SUBJECT.stage("windows", output)
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
