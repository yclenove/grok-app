"""Linux proposal contract oracles; synthetic fixtures are not browser proof."""
import hashlib
import importlib.util
import io
import lzma
import struct
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "linux_proposal", Path(__file__).with_name("inspect-ungoogled-linux-sources.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)


def elf(machine=62):
    contents = bytearray(64)
    contents[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", contents, 18, machine)
    return bytes(contents)


def pack(contents):
    # One v5 UTF-8 resource and canonical end sentinel.
    header = struct.pack("<IBxxxHH", 5, 1, 1, 0)
    start = len(header) + 12
    return header + struct.pack("<HIHI", 1, start, 0, start + len(contents)) + contents


FULL = (b"<html><title>Credits</title>Chromium Copyright license " + b"notice " * 700 + b"</html>")
STUB = b"<html><title>Credits</title>This is sample credits page. generate_about_credits=true</html>"


def archive(entries):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w", format=tarfile.GNU_FORMAT) as stream:
        for name, data, kind in entries:
            entry = tarfile.TarInfo(name)
            entry.size, entry.type = len(data), kind
            stream.addfile(entry, io.BytesIO(data))
    return lzma.compress(output.getvalue())


class LinuxProposalTests(unittest.TestCase):
    def default_entries(self, document=FULL, machine=62):
        return [(SUBJECT.ROOT + "/", b"", tarfile.DIRTYPE),
                (SUBJECT.ROOT + "/chrome", elf(machine), tarfile.REGTYPE),
                (SUBJECT.ROOT + "/resources.pak", pack(document), tarfile.REGTYPE)]

    def inspect(self, entries, pin=True):
        contents = archive(entries)
        with tempfile.TemporaryDirectory(prefix="grok-cu-linux-source-oracle-") as directory:
            path = Path(directory) / "Linux_x64.tar.xz"
            path.write_bytes(contents)
            with patch.object(SUBJECT, "ARCHIVE_BYTES", len(contents)), \
                    patch.object(SUBJECT, "SHA256", hashlib.sha256(contents).hexdigest() if pin else SUBJECT.SHA256):
                return SUBJECT.inspect_directory(Path(directory))

    def test_full_catalog_and_tree_still_do_not_certify_native_abi_or_redistribution(self):
        report, payloads = self.inspect(self.default_entries())
        self.assertEqual(report["embeddedFullNoticesGate"], "candidate_only_not_certified")
        self.assertEqual(payloads, [[FULL]])
        self.assertEqual((report["nativeAcceptance"], report["glibcAcceptance"]), ("not_run", "not_run"))
        self.assertEqual((report["executed"], report["promoted"]), (False, False))
        row = report["archives"][0]
        self.assertEqual((row["treeFiles"], row["directories"], row["archiveEntries"]), (2, 1, 3))
        self.assertEqual(row["treeBytes"], len(elf()) + len(pack(FULL)))
        self.assertEqual(row["nativeHeaders"]["chrome"]["formatArchitecture"], "elf-x86_64")

    def test_wrong_pin_rejects_before_tar_inspection(self):
        with self.assertRaisesRegex(ValueError, "SHA256"):
            self.inspect(self.default_entries(), pin=False)

    def test_sample_document_remains_missing_notices(self):
        report, _ = self.inspect(self.default_entries(STUB))
        self.assertEqual(report["embeddedFullNoticesGate"], "failed_missing_full_notices")
        self.assertFalse(report["promoted"])

    def test_traversal_colon_wrong_root_and_case_duplicate_reject(self):
        for name in (SUBJECT.ROOT + "/../bad", SUBJECT.ROOT + "/C:bad", "wrong/chrome",
                     SUBJECT.ROOT + "/CHROME"):
            entries = self.default_entries() + [(name, b"bad", tarfile.REGTYPE)]
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.inspect(entries)

    def test_missing_browser_or_credits_and_wrong_native_architecture_reject(self):
        without_browser = [row for row in self.default_entries() if not row[0].endswith("/chrome")]
        for entries in (without_browser, self.default_entries()[:2],
                        self.default_entries(machine=183)):
            with self.subTest(names=[row[0] for row in entries]), self.assertRaises(ValueError):
                self.inspect(entries)

    def test_pack_budget_and_malformed_pack_reject(self):
        with patch.object(SUBJECT, "MAX_PACK_BYTES", 2), self.assertRaisesRegex(ValueError, "budget"):
            self.inspect(self.default_entries())
        entries = self.default_entries()[:2] + [(SUBJECT.ROOT + "/resources.pak", b"not a pack", tarfile.REGTYPE)]
        with self.assertRaisesRegex(ValueError, "DataPack"):
            self.inspect(entries)


if __name__ == "__main__":
    unittest.main()
