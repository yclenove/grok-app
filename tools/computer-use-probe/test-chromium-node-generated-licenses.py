"""Pinned GCS source identity and bounded inert node source collection."""
import copy
import gzip
import hashlib
import io
import json
from pathlib import Path
import runpy
import tarfile
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-node-generated-licenses.py")))


def archive_bytes(entries):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for name, data, kind, link in entries:
            row = tarfile.TarInfo(name)
            row.size, row.type, row.linkname = len(data), kind, link
            archive.addfile(row, io.BytesIO(data))
    return gzip.compress(stream.getvalue())


class NodeGeneratedLicenseTests(unittest.TestCase):
    def metadata(self):
        return {"kind": "storage#object", "bucket": "chromium-nodejs", "name": SUBJECT["OBJECT"]["object_name"],
                "generation": str(SUBJECT["OBJECT"]["generation"]), "size": str(SUBJECT["OBJECT"]["size_bytes"])}

    def test_exact_immutable_declaration_and_generation_metadata(self):
        original = self.metadata()
        self.assertEqual(SUBJECT["metadata"](json.dumps(original).encode()), original)
        self.assertIn("?generation=" + original["generation"], SUBJECT["ARCHIVE_URL"])
        self.assertEqual(SUBJECT["DECLARATION"]["objects"], [SUBJECT["OBJECT"]])
        for key, value in (("bucket", "other"), ("name", "latest"), ("generation", int(original["generation"])),
                           ("size", "1"), ("kind", "other")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["metadata"](json.dumps({**original, key: value}).encode())

    def test_metadata_empty_or_byte_budget_reject(self):
        for data in (b"", b" " * (SUBJECT["MAX_METADATA"] + 1)):
            with self.assertRaises(ValueError):
                SUBJECT["metadata"](data)

    def test_canonical_archive_paths_and_single_dot_prefix(self):
        self.assertEqual(SUBJECT["archive_path"]("./node_modules/lit/LICENSE"), "node_modules/lit/LICENSE")
        for name in ("../escape", "node_modules//file", "././node_modules/file", "C:/file", "node_modules\\file", "node_modules/\x01"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["archive_path"](name)

    def test_link_paths_are_bounded_inert_and_cannot_escape_root(self):
        self.assertEqual(SUBJECT["link_target"]("node_modules/.bin/tsc", "../typescript/bin/tsc", True), "node_modules/typescript/bin/tsc")
        self.assertEqual(SUBJECT["link_target"]("node_modules/link", "node_modules/file", False), "node_modules/file")
        for target in ("../../escape", "/absolute", "C:/file", "other\\file", ""):
            with self.subTest(target=target), self.assertRaises(ValueError):
                SUBJECT["link_target"]("node_modules/link", target, True)

    def inspect(self, root, entries, references=None):
        path = root / "archive.gz-data"
        # Original GCS tar is contents-relative, with a '.' directory entry.
        relative = [("." if name == "node_modules" else name[len("node_modules/"):] if name.startswith("node_modules/") else name, data, kind, link)
                    for name, data, kind, link in entries]
        path.write_bytes(archive_bytes(relative))
        return SUBJECT["inspect_archive"](path, root, references or [])

    def test_license_data_versions_and_regular_files_selected_without_extraction(self):
        rows = [("node_modules", b"", tarfile.DIRTYPE, ""),
                ("node_modules/lit/LICENSE", b"license", tarfile.REGTYPE, ""),
                ("node_modules/lit/package.json", b'{"name":"lit","version":"3.3.0"}', tarfile.REGTYPE, ""),
                ("node_modules/.bin/tool", b"", tarfile.SYMTYPE, "../lit/script.js"),
                ("node_modules/lit/script.js", b"throw new Error('not executed');", tarfile.REGTYPE, "")]
        ref = {"sourcePath": SUBJECT["PREFIX"] + "/lit/LICENSE"}
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            root = Path(name)
            result = self.inspect(root, rows, [ref])
            self.assertEqual(result["entryKinds"], {"directory": 1, "file": 3, "symlink": 1})
            self.assertEqual(result["missingRequestedPaths"], [])
            self.assertEqual(len(result["selectedFiles"]), 2)
            self.assertFalse((root / "node_modules").exists())
            self.assertFalse(result["extracted"])
            self.assertFalse(result["linksFollowed"])
            license_row = result["selectedFiles"][0]
            self.assertEqual((root / license_row["sourceFile"]).read_bytes(), b"license")
            self.assertEqual(license_row["sha256"], hashlib.sha256(b"license").hexdigest())

    def test_selected_links_unsafe_roots_and_duplicate_or_case_colliding_names_reject(self):
        fixtures = [[("node_modules/lit/LICENSE", b"", tarfile.SYMTYPE, "../other/LICENSE")],
                    [("../outside/file", b"data", tarfile.REGTYPE, "")],
                    [("node_modules/a", b"a", tarfile.REGTYPE, ""), ("node_modules/a", b"a", tarfile.REGTYPE, "")],
                    [("node_modules/a", b"a", tarfile.REGTYPE, ""), ("node_modules/A", b"a", tarfile.REGTYPE, "")]]
        for rows in fixtures:
            with self.subTest(rows=rows), tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name, self.assertRaises(ValueError):
                self.inspect(Path(name), rows)

    def test_missing_original_reference_remains_explicit(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            result = self.inspect(Path(name), [("node_modules/other", b"data", tarfile.REGTYPE, "")],
                                  [{"sourcePath": SUBJECT["PREFIX"] + "/lit/LICENSE"}])
            self.assertEqual(result["missingRequestedPaths"], ["node_modules/lit/LICENSE"])

    def test_selected_payload_budget_is_checked_before_read(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name, \
                patch.dict(SUBJECT["inspect_archive"].__globals__, MAX_SELECTED_FILE=3), self.assertRaisesRegex(ValueError, "file byte budget"):
            self.inspect(Path(name), [("node_modules/lit/LICENSE", b"license", tarfile.REGTYPE, "")])

    def test_identical_license_bytes_share_inert_blob_but_keep_distinct_paths(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            rows = [("node_modules/one/LICENSE", b"same", tarfile.REGTYPE, ""), ("node_modules/two/LICENSE", b"same", tarfile.REGTYPE, "")]
            result = self.inspect(Path(name), rows)
            self.assertEqual(len(result["selectedFiles"]), 2)
            self.assertEqual(len({row["sourceFile"] for row in result["selectedFiles"]}), 1)

    def test_archive_and_lock_version_differences_are_preserved_not_normalized(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            root = Path(name)
            rows = [("node_modules/lit/package.json", b'{"name":"lit","version":"3.3.0"}', tarfile.REGTYPE, ""),
                    ("node_modules/@azure/msal-browser/package.json", b'{"name":"@azure/msal-browser","version":"4.2.1","license":"MIT"}', tarfile.REGTYPE, "")]
            report = self.inspect(root, rows)
            lock = {"lockfileVersion": 2, "packages": {"node_modules/lit": {"version": "3.2.1"},
                    "node_modules/@azure/msal-browser": {"version": "4.2.1", "license": "MIT"}}}
            original = copy.deepcopy(lock)
            result = SUBJECT["package_comparison"](report, root, {"third_party/node/package-lock.json": json.dumps(lock).encode()})
            self.assertFalse(result[0]["versionMatchesLock"])
            self.assertTrue(result[1]["versionMatchesLock"])
            self.assertEqual(lock, original)
            self.assertFalse(result[0]["originalReadmeVersionOverridden"])

    def test_foreign_existing_or_escaping_output_reject_before_network(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            root = Path(name)
            for output in (root / "other", root / "../node-generated-license-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, output)
            output = root / "node-generated-license-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_late_archive_failure_keeps_metadata_and_red_partial_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            root = Path(name)
            def fail(_):
                raise ValueError("fixture archive failure")
            with patch.dict(SUBJECT["collect"].__globals__, checked_sources=lambda *_: ([], {}, "fixture", {}),
                            fetch_metadata=lambda: json.dumps(self.metadata()).encode(), download=fail), self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "node-generated-license-01")
            report = json.loads((root / "node-generated-license-01/node-generated-inventory.json").read_text())
            self.assertFalse(report["collectionComplete"])
            self.assertTrue((root / "node-generated-license-01/object-metadata.json-data").is_file())
            for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "packageManagerExecuted", "actualBuilderInputMeasured", "promoted"):
                self.assertFalse(report[key])

    def test_parent_file_or_link_conflicts_reject_in_both_entry_orders(self):
        for rows in ([("node_modules/parent", b"file", tarfile.REGTYPE, ""),
                      ("node_modules/parent/file", b"child", tarfile.REGTYPE, "")],
                     [("node_modules/parent/file", b"child", tarfile.REGTYPE, ""),
                      ("node_modules/parent", b"file", tarfile.REGTYPE, "")]):
            with self.subTest(rows=rows), tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name, self.assertRaises(ValueError):
                self.inspect(Path(name), rows)

    def test_invalid_original_helper_or_foreign_cache_generation_reject(self):
        with self.assertRaisesRegex(ValueError, "helper SHA256"):
            SUBJECT["gcs_layout"](b"foreign source")
        for name in ("../node-generated-license-01", "latest", "node-generated-license-001"):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "safe existing"):
                SUBJECT["cached_archive"](Path("fixture"), name)

    def test_original_archive_cache_bytes_must_match_declared_size_and_hash(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-node-") as name:
            path = Path(name) / "archive.gz-data"
            path.write_bytes(b"unchanged fixture")
            original = {**SUBJECT["OBJECT"], "size_bytes": path.stat().st_size,
                        "sha256sum": hashlib.sha256(path.read_bytes()).hexdigest()}
            with patch.dict(SUBJECT["archive_identity"].__globals__, OBJECT=original):
                self.assertEqual(SUBJECT["archive_identity"](path)["sha256"], original["sha256sum"])
                path.write_bytes(b"changed__ fixture")
                with self.assertRaises(ValueError):
                    SUBJECT["archive_identity"](path)


if __name__ == "__main__":
    unittest.main()
