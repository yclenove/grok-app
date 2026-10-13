"""Small source-inventory oracles; never run/extract a downloaded runtime."""
import base64
import hashlib
import io
import json
import plistlib
import runpy
import struct
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch


INSPECTOR = runpy.run_path(str(Path(__file__).with_name("inspect-maintained-node-sources.py")))
INSPECT = INSPECTOR["inspect"]
HEADER = INSPECTOR["native_header"]
CHROME_INSPECT = runpy.run_path(str(Path(__file__).with_name(
    "inspect-maintained-chromium-sources.py")))["inspect"]
PW_INSPECT = runpy.run_path(str(Path(__file__).with_name(
    "inspect-maintained-playwright-sources.py")))["inspect"]
SNAPSHOT_INSPECTOR = runpy.run_path(str(Path(__file__).with_name(
    "inspect-unbranded-chromium-sources.py")))
LICENSE = b"Permission is hereby granted, free of charge\ncomplete third-party notices\n"


class UnbrandedSnapshotTests(unittest.TestCase):
    def source(self, contents, platform="Win_x64"):
        root = SNAPSHOT_INSPECTOR["SNAPSHOTS"][platform][0]
        return {"platform": platform, "sourceCommit": SNAPSHOT_INSPECTOR["COMMIT"],
                "sourcePosition": SNAPSHOT_INSPECTOR["POSITION"], "archive": {
                    "name": f"{platform}/1716663/{root}.zip", "generation": "12345",
                    "size": str(len(contents)),
                    "md5Hash": base64.b64encode(hashlib.md5(contents).digest()).decode()}}

    def inspect(self, contents, source, platform="Win_x64"):
        with tempfile.TemporaryDirectory(prefix="grok-cu-snapshot-oracle-") as directory:
            path = Path(directory) / "candidate.zip"
            path.write_bytes(contents)
            return SNAPSHOT_INSPECTOR["inspect_archive"](path, platform, source)

    def test_same_source_inventory_is_never_runtime_or_publication_approval(self):
        contents = zip_bytes([("chrome-win/chrome.exe", pe())])
        result = self.inspect(contents, self.source(contents))
        self.assertEqual(result["nativeHeaders"]["chrome.exe"]["formatArchitecture"], "pe-x86_64")
        self.assertTrue(result["officialObjectMd5Matched"])
        self.assertFalse(result["executed"])
        self.assertFalse(result["promoted"])

    def test_source_position_commit_object_and_integrity_mismatch_reject(self):
        contents = zip_bytes([("chrome-win/chrome.exe", pe())])
        for field in ("sourceCommit", "sourcePosition", "platform", "name", "generation", "size", "md5Hash"):
            source = self.source(contents)
            if field in source:
                source[field] = "wrong"
            else:
                source["archive"][field] = "0" if field == "size" else "wrong"
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.inspect(contents, source)

    def test_wrong_native_architecture_does_not_pass_inventory(self):
        contents = zip_bytes([("chrome-linux/chrome", pe())])
        with self.assertRaisesRegex(ValueError, "architecture"):
            self.inspect(contents, self.source(contents, "Linux_x64"), "Linux_x64")

    def test_mac_executable_and_framework_must_both_match_source_target(self):
        info = plistlib.dumps({"CFBundleShortVersionString": "157.0.8101.0", "LSMinimumSystemVersion": "13.0"})
        prefix = "chrome-mac/Chromium.app/Contents/"
        for framework_cpu, valid in ((0x0100000C, True), (0x01000007, False)):
            contents = zip_bytes([(prefix + "Info.plist", info),
                                  (prefix + "MacOS/Chromium", macho(0x0100000C)),
                                  (prefix + "Frameworks/Chromium Framework.framework/Versions/"
                                   "157.0.8101.0/Chromium Framework", macho(framework_cpu))])
            if valid:
                result = self.inspect(contents, self.source(contents, "Mac_Arm"), "Mac_Arm")
                self.assertEqual(result["macosMinimumSystemMetadata"]["plistMinimumSystemVersion"], "13.0")
                self.assertFalse(result["executed"])
            else:
                with self.assertRaisesRegex(ValueError, "framework architecture"):
                    self.inspect(contents, self.source(contents, "Mac_Arm"), "Mac_Arm")


def pe():
    data = bytearray(128)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, 0x8664)
    struct.pack_into("<H", data, 88, 0x20B)
    return bytes(data)


def elf():
    data = bytearray(64)
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", data, 18, 62)
    return bytes(data)


def macho(cpu):
    return (struct.pack("<IIIIIIII", 0xFEEDFACF, cpu, 0, 2, 1, 24, 0, 0)
            + struct.pack("<IIIIII", 0x32, 24, 1, (13 << 16) | (5 << 8), 0, 0))


def zip_bytes(entries):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, contents in entries:
            archive.writestr(name, contents)
    return output.getvalue()


def inspect_bytes(contents, suffix=".zip", executable="node.exe", architecture="pe-x86_64",
                  expected=None):
    with tempfile.TemporaryDirectory(prefix="grok-cu-node-inventory-") as directory:
        path = Path(directory) / f"source{suffix}"
        path.write_bytes(contents)
        return INSPECT(path, "root", executable, architecture,
                       expected if expected is not None else hashlib.sha256(contents).hexdigest())


class NodeInventoryTests(unittest.TestCase):
    def test_four_native_headers_and_macos_minimum(self):
        samples = [(pe(), "pe-x86_64"), (elf(), "elf-x86_64"),
                   (macho(0x0100000C), "macho-arm64"), (macho(0x01000007), "macho-x86_64")]
        for binary, architecture in samples:
            self.assertEqual(HEADER(binary)["formatArchitecture"], architecture)
        self.assertEqual(HEADER(samples[2][0])["headerMinimumMacOS"], "13.5.0")

    def test_truncated_and_wrong_native_headers_reject(self):
        for binary in (b"not native", pe()[:63], macho(0x0100000C)[:-1], macho(9)):
            with self.assertRaises(ValueError):
                HEADER(binary)

    def test_zip_directory_and_nonselected_files_are_counted(self):
        result = inspect_bytes(zip_bytes([
            ("root/", b""), ("root/sub/", b""), ("root/node.exe", pe()),
            ("root/LICENSE", LICENSE), ("root/sub/ignored", b"unextracted"),
        ]))
        self.assertEqual(result["entries"], 5)
        self.assertEqual(result["entryTypes"], {"directory": 2, "regular": 3})
        self.assertEqual(result["executable"]["sha256"], hashlib.sha256(pe()).hexdigest())

    def test_windows_license_line_endings_do_not_hide_content_changes(self):
        def license_row(contents):
            return inspect_bytes(zip_bytes([
                ("root/node.exe", pe()), ("root/LICENSE", contents),
            ]))["license"]
        lf, crlf = license_row(LICENSE), license_row(LICENSE.replace(b"\n", b"\r\n"))
        changed = license_row(LICENSE + b"different notice")
        self.assertNotEqual(lf["sha256"], crlf["sha256"])
        self.assertEqual(lf["lfComparisonSha256"], crlf["lfComparisonSha256"])
        self.assertNotEqual(lf["lfComparisonSha256"], changed["lfComparisonSha256"])

    def test_hash_architecture_and_missing_license_reject(self):
        complete = zip_bytes([("root/node.exe", pe()), ("root/LICENSE", LICENSE)])
        with self.assertRaisesRegex(ValueError, "SHASUMS256 mismatch"):
            inspect_bytes(complete, expected="0" * 64)
        with self.assertRaisesRegex(ValueError, "does not match target"):
            inspect_bytes(complete, architecture="macho-arm64")
        with self.assertRaisesRegex(ValueError, "complete LICENSE"):
            inspect_bytes(zip_bytes([("root/node.exe", pe())]))

    def test_duplicate_and_unexpected_paths_reject(self):
        for extra in ["root/NODE.exe", "root/../outside", "other/", "root/sub//file"]:
            with self.assertRaises(ValueError):
                inspect_bytes(zip_bytes([("root/node.exe", pe()), ("root/LICENSE", LICENSE),
                                         (extra, b"")]))

    def test_entry_and_byte_quotas_include_skipped_payloads(self):
        contents = zip_bytes([("root/node.exe", pe()), ("root/LICENSE", LICENSE),
                              ("root/skip", b"ignored payload")])
        for bound, limit in [("MAX_ENTRIES", 2), ("MAX_BYTES", 128)]:
            with patch.dict(INSPECT.__globals__, {bound: limit}):
                with self.assertRaisesRegex(ValueError, "budget exceeded"):
                    inspect_bytes(contents)

    def test_tar_skipped_package_links_are_not_native_payloads(self):
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for name, contents in [("root/bin/node", elf()), ("root/LICENSE", LICENSE)]:
                entry = tarfile.TarInfo(name)
                entry.size = len(contents)
                archive.addfile(entry, io.BytesIO(contents))
            entry = tarfile.TarInfo("root/bin/npm")
            entry.type, entry.linkname = tarfile.SYMTYPE, "../lib/node_modules/npm/bin/npm-cli.js"
            archive.addfile(entry)
        result = inspect_bytes(output.getvalue(), ".tar.gz", "bin/node", "elf-x86_64")
        self.assertEqual(result["entryTypes"]["unextracted-symlink"], 1)


class BrowserInventoryTests(unittest.TestCase):
    def inspect_browser(self, entries, root="chrome-linux64", executable="chrome"):
        with tempfile.TemporaryDirectory(prefix="grok-cu-browser-inventory-") as directory:
            path = Path(directory) / f"{root}.zip"
            path.write_bytes(zip_bytes(entries))
            return CHROME_INSPECT(path, root, executable)

    def test_directories_participate_in_inventory_and_duplicates(self):
        result = self.inspect_browser([("chrome-linux64/", b""), ("chrome-linux64/sub/", b""),
                                       ("chrome-linux64/chrome", b"browser")])
        self.assertEqual(result["archiveEntries"], 3)
        self.assertEqual(result["entryTypes"], {"directory": 2, "regular": 1})
        self.assertEqual(result["treeFiles"], 1)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            self.inspect_browser([("chrome-linux64/sub/", b""), ("chrome-linux64/SUB/", b"")])

    def test_directory_payloads_and_ambiguous_paths_reject(self):
        for name, contents in [("chrome-linux64/sub/", b"payload"),
                               ("chrome-linux64//file", b""), ("other/", b"")]:
            with self.assertRaises(ValueError):
                self.inspect_browser([(name, contents)])

    def test_macos_binary_headers_and_original_archive_identity(self):
        root = "chrome-mac-arm64"
        base = f"{root}/Google Chrome for Testing.app/Contents/"
        executable = "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
        framework = (base + "Frameworks/Google Chrome for Testing Framework.framework/"
                     "Versions/156.0.8078.4/Google Chrome for Testing Framework")
        entries = [(base + "Info.plist", plistlib.dumps({"LSMinimumSystemVersion": "13.5"})),
                   (f"{root}/{executable}", macho(0x0100000C)),
                   (framework, macho(0x0100000C))]
        result = self.inspect_browser(entries, root, executable)
        self.assertEqual(result["archive"], root + ".zip")
        metadata = result["macosMinimumSystemMetadata"]
        self.assertEqual(metadata["plistMinimumSystemVersion"], "13.5")
        self.assertEqual(len(metadata["nativeHeaderRequirements"]), 2)
        entries[-1] = (framework, macho(0x01000007))
        with self.assertRaisesRegex(ValueError, "architecture does not match"):
            self.inspect_browser(entries, root, executable)


class PlaywrightInventoryTests(unittest.TestCase):
    def entries(self):
        return [
            ("package/LICENSE", b"Apache License, complete fixture grant"),
            ("package/NOTICE", b"notice"), ("package/ThirdPartyNotices.txt", b"third party"),
            ("package/package.json", json.dumps({"name": "playwright-core", "version": "1.64.0",
                                                "engines": {"node": ">=20"}}).encode()),
            ("package/browsers.json", b'{"browsers": []}'),
        ]

    def inspect_package(self, entries=None, metadata_change=None, overrides=None, link=False):
        entries = self.entries() if entries is None else entries
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for name, contents in entries:
                entry = tarfile.TarInfo(name)
                entry.size = len(contents)
                archive.addfile(entry, io.BytesIO(contents))
            if link:
                entry = tarfile.TarInfo("package/linked")
                entry.type, entry.linkname = tarfile.SYMTYPE, "LICENSE"
                archive.addfile(entry)
        raw = output.getvalue()
        metadata = {"name": "playwright-core", "version": "1.64.0", "dist": {
            "integrity": "sha512-" + base64.b64encode(hashlib.sha512(raw).digest()).decode(),
            "fileCount": len(entries), "unpackedSize": sum(len(contents) for _, contents in entries),
        }}
        if metadata_change:
            metadata_change(metadata)
        with tempfile.TemporaryDirectory(prefix="grok-cu-pw-inventory-") as directory:
            path = Path(directory)
            (path / "playwright-core-1.64.0.tgz").write_bytes(raw)
            (path / "playwright-core-1.64.0.metadata.json").write_text(json.dumps(metadata), encoding="utf-8")
            # Only synthetic oracle inputs override these fixed inspector
            # hashes. Product compiled pins/locks and the real audit do not.
            fields = {"SHA256": hashlib.sha256(raw).hexdigest(), "ARCHIVE_BYTES": len(raw)}
            fields.update(overrides or {})
            with patch.dict(PW_INSPECT.__globals__, fields):
                return PW_INSPECT(path)

    def test_full_tree_and_original_notices_are_fingerprinted(self):
        result = self.inspect_package()
        self.assertEqual(result["treeFiles"], 5)
        self.assertEqual(result["notices"]["NOTICE"]["sha256"], hashlib.sha256(b"notice").hexdigest())
        self.assertTrue(result["npmSriMatched"])
        rows = sorted((name[len("package/"):], contents) for name, contents in self.entries())
        listing = "".join(f"{name}\t{len(contents)}\t{hashlib.sha256(contents).hexdigest()}\n"
                          for name, contents in rows)
        self.assertEqual(result["treeSha256"], hashlib.sha256(listing.encode()).hexdigest())

    def test_hash_and_metadata_integrity_are_not_interchangeable(self):
        with self.assertRaisesRegex(ValueError, "hash/size"):
            self.inspect_package(overrides={"SHA256": "0" * 64})
        with self.assertRaisesRegex(ValueError, "identity/SRI"):
            self.inspect_package(metadata_change=lambda metadata: metadata["dist"].update(integrity="wrong"))

    def test_npm_count_and_bytes_must_match_all_files(self):
        for field in ("fileCount", "unpackedSize"):
            with self.assertRaisesRegex(ValueError, "count/size"):
                self.inspect_package(metadata_change=lambda metadata: metadata["dist"].update({field: 0}))

    def test_missing_notices_cannot_pass_a_matching_metadata_tree(self):
        with self.assertRaisesRegex(ValueError, "complete package notices"):
            self.inspect_package([entry for entry in self.entries() if entry[0] != "package/NOTICE"])

    def test_paths_duplicates_and_archive_links_fail_closed(self):
        for name in ("package/../escape", "other/file", "package//ambiguous", "package/notice"):
            with self.assertRaises(ValueError):
                self.inspect_package(self.entries() + [(name, b"extra")])
        with self.assertRaisesRegex(ValueError, "links/special"):
            self.inspect_package(link=True)

    def test_actual_engine_and_package_identity_are_checked(self):
        for change in ({"engines": {"node": ">=99"}}, {"version": "0.0.1"}, {"name": "other"}):
            entries = self.entries()
            package = json.loads(entries[3][1])
            package.update(change)
            entries[3] = (entries[3][0], json.dumps(package).encode())
            with self.assertRaisesRegex(ValueError, "metadata/engine"):
                self.inspect_package(entries)

    def test_byte_entry_and_path_quotas_cover_unselected_files(self):
        for field, limit in (("MAX_FILES", 4), ("MAX_BYTES", 5), ("MAX_FILE_BYTES", 3), ("MAX_PATH", 5)):
            with self.assertRaises(ValueError):
                self.inspect_package(overrides={field: limit})


if __name__ == "__main__":
    unittest.main()
