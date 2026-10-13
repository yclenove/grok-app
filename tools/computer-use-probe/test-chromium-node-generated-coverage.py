"""Generated source has its own identity; original log/closure evidence stays honest."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-node-generated-coverage.py")))


class NodeGeneratedCoverageTests(unittest.TestCase):
    def section(self):
        prefix = "src/" + SUBJECT["NODE"]["PREFIX"] + ":" + SUBJECT["NODE"]["OBJECT"]["object_name"]
        return (prefix + " (Elapsed: 0:00:00)\n" + "-" * 40 + "\n[0:00:08] Started.\n[0:00:08] Finished.\n" + "-" * 40 + "\n\n")

    def test_original_same_object_finished_section_and_line_recorded(self):
        row = SUBJECT["checkout_section"](("unrelated prefix\n" + self.section()).encode())
        self.assertEqual(row["headerLine"], 2)
        self.assertTrue(row["exactObjectCheckoutRecorded"])
        self.assertFalse(row["remoteArchiveBytesMeasured"])
        self.assertFalse(row["binaryCoverageProven"])

    def test_foreign_duplicate_unfinished_or_partial_log_section_reject(self):
        original = self.section()
        for value in (original.replace(SUBJECT["NODE"]["OBJECT"]["object_name"], "a" * 40),
                      original + original, original.replace("Finished.", "Failed."), original[:120]):
            with self.subTest(length=len(value)), self.assertRaises(ValueError):
                SUBJECT["checkout_section"](value.encode())

    def fixture(self, root, data=b"actual license"):
        path = "third_party/node/node_modules/lit/LICENSE"
        digest = hashlib.sha256(data).hexdigest()
        (root / (digest + ".source-data")).write_bytes(data)
        report = {"inspection": {"entries": [{"path": "node_modules/lit/LICENSE", "kind": "file",
                                              "bytes": len(data), "sha256": digest}],
                    "selectedFiles": [{"sourcePath": path, "bytes": len(data), "sha256": digest,
                                       "sourceFile": digest + ".source-data"}]}}
        return path, report

    def test_generated_archive_does_not_fabricate_git_mode_or_object_identity(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-coverage-") as name:
            root = Path(name)
            path, report = self.fixture(root)
            inputs = {"index": {}, "files": {}}
            SUBJECT["append_node"](inputs, root, report)
            row = inputs["index"][path]
            self.assertEqual(row["mode"], "archive_regular_file")
            self.assertEqual(row["kind"], "generated_archive_input")
            self.assertNotIn("gitObjectSha1", row)
            view = SUBJECT["GeneratedSourceView"](inputs["index"], {path: inputs["files"][path]["payload"]})
            self.assertTrue(view.regular(path))
            self.assertEqual(SUBJECT["resolution"]({"sourcePath": path}, inputs), "resolved_to_verified_source_bytes")

    def test_original_git_source_collision_reject_without_overwrite(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-coverage-") as name:
            root = Path(name)
            path, report = self.fixture(root)
            original = {"mode": "100644", "gitObjectSha1": "a" * 40}
            inputs = {"index": {path: original.copy()}, "files": {}}
            with self.assertRaisesRegex(ValueError, "overwrite Git"):
                SUBJECT["append_node"](inputs, root, report)
            self.assertEqual(inputs["index"][path], original)

    def test_mutated_selected_bytes_and_identity_reject(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-coverage-") as name:
            root = Path(name)
            _, report = self.fixture(root)
            (root / report["inspection"]["selectedFiles"][0]["sourceFile"]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "source bytes changed"):
                SUBJECT["append_node"]({"index": {}, "files": {}}, root, report)

    def test_archive_missing_bytes_and_non_regular_git_or_remote_resolution_stay_open(self):
        inputs = {"index": {"generated": {"mode": "archive_regular_file"}, "link": {"mode": "120000"}}, "files": {}}
        for path, expected in (("generated", "regular_archive_bytes_not_collected"),
                               ("link", "reference_is_not_regular_source_file"), (None, "remote_reference_not_followed")):
            self.assertEqual(SUBJECT["resolution"]({"sourcePath": path}, inputs), expected)

    def test_selected_archive_missing_blob_refuses_reverification_write(self):
        import gzip
        import io
        import tarfile
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-coverage-") as name:
            root = Path(name)
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w", format=tarfile.GNU_FORMAT) as archive:
                row = tarfile.TarInfo("lit/LICENSE")
                row.size = 4
                archive.addfile(row, io.BytesIO(b"text"))
            path = root / "archive.gz-data"
            path.write_bytes(gzip.compress(raw.getvalue()))
            with self.assertRaisesRegex(ValueError, "source bytes absent"):
                SUBJECT["NODE"]["inspect_archive"](path, root, [], require_existing=True)
            self.assertEqual(list(root.iterdir()), [path])

    def test_foreign_existing_or_escaping_output_refuses_before_inputs(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-generated-coverage-") as name:
            root = Path(name)
            for output in (root / "other.json", root / "../node-generated-coverage-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](root, output)
            output = root / "node-generated-coverage-01.json"
            output.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"original")

    def test_static_policy_uses_generated_license_bytes_without_deleting_other_errors(self):
        index = {"third_party/node/README.chromium": {"mode": "100644"},
                 "third_party/node/node_modules/lit/LICENSE": {"mode": "archive_regular_file"}}
        payloads = {"third_party/node/node_modules/lit/LICENSE": b"actual license"}
        view = SUBJECT["GeneratedSourceView"](index, payloads)
        policy = {"ALIAS_FIELDS": {}, "NOT_SHIPPED": "NOT_SHIPPED", "YES": "yes", "NO": "no",
                  "MANDATORY_FIELDS": {"Name", "URL", "License"}}
        metadata = {"Name": "Lit", "URL": "https://lit.dev", "License": "BSD-3-Clause", "Shipped": "yes",
                    "License File": ["node_modules/lit/LICENSE"]}
        before = metadata.copy()
        result = SUBJECT["POLICY"]["process"](metadata, "third_party/node", view, policy)
        self.assertEqual(result["errors"], [])
        self.assertEqual(result["availableLicensePaths"], ["third_party/node/node_modules/lit/LICENSE"])
        self.assertEqual(metadata, before)
        metadata["License File"] = ["node_modules/missing/LICENSE"]
        self.assertTrue(SUBJECT["POLICY"]["process"](metadata, "third_party/node", view, policy)["errors"])


if __name__ == "__main__":
    unittest.main()
