"""Perfetto collection rejects foreign pins and retains failed source evidence."""
import copy
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-perfetto-licenses.py")))


class PerfettoSourceTests(unittest.TestCase):
    def fixture(self):
        prefix, repository, revision = SUBJECT["PINS"]["perfetto"]
        inputs = {"mainSha256": "main", "references": [],
                  "index": {prefix: {"mode": "160000", "gitObjectSha1": revision}}}
        catalog = {"gitDependencies": [{"path": prefix, "repository": repository, "revision": revision,
                    "condition": None, "gitlinkRevision": revision, "gitlinkMatched": True}]}
        checkout = {"targets": [{"target": name, "manifest": {"name": name}} for name in
                               ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")]}
        return inputs, catalog, checkout

    def checked(self, inputs, catalog, checkout, compare):
        with patch.dict(SUBJECT["INTEGRATION"], checked_inputs=lambda *_: (inputs, "checkout", [])), \
                patch.dict(SUBJECT["INPUTS"], read_data=lambda *_: b"sealed catalog"), \
                patch.dict(SUBJECT["EXTERNAL"], checked_catalog=lambda *_: catalog), \
                patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda *_: checkout, compare_pin=compare):
            return SUBJECT["checked_inputs"](Path("fixture"))

    def test_pin_must_match_all_four_actual_manifests(self):
        inputs, catalog, checkout = self.fixture()
        calls = []
        def compare(path, repository, revision, manifest):
            calls.append((path, repository, revision, manifest["name"]))
            return {"status": "exact_repository_and_revision_match"}
        original = copy.deepcopy((inputs, catalog, checkout))
        result, sha, rows = self.checked(inputs, catalog, checkout, compare)
        self.assertIs(result, inputs)
        self.assertEqual(sha, "checkout")
        self.assertEqual(len(rows), 4)
        self.assertEqual({row["target"] for row in rows}, {row["target"] for row in checkout["targets"]})
        prefix, repository, revision = SUBJECT["PINS"]["perfetto"]
        self.assertTrue(all(row[:3] == ("src/" + prefix, repository, revision) for row in calls))
        self.assertEqual((inputs, catalog, checkout), original)

    def test_conditional_duplicate_or_foreign_deps_refuses_before_manifest_read(self):
        for change in ({"condition": "checkout_perfetto"}, {"revision": "a" * 40},
                       {"repository": "https://example.test/perfetto"}, {"gitlinkMatched": False}):
            inputs, catalog, checkout = self.fixture()
            catalog["gitDependencies"][0].update(change)
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "DEPS declaration"):
                self.checked(inputs, catalog, checkout, lambda *_: self.fail("manifest must not be read"))
        inputs, catalog, checkout = self.fixture()
        catalog["gitDependencies"] *= 2
        with self.assertRaisesRegex(ValueError, "DEPS declaration"):
            self.checked(inputs, catalog, checkout, lambda *_: self.fail("manifest must not be read"))

    def test_main_tree_missing_foreign_or_regular_blob_cannot_replace_gitlink(self):
        prefix = SUBJECT["PINS"]["perfetto"][0]
        for entry in (None, {"mode": "100644", "gitObjectSha1": SUBJECT["PINS"]["perfetto"][2]},
                      {"mode": "160000", "gitObjectSha1": "b" * 40}):
            inputs, catalog, checkout = self.fixture()
            inputs["index"] = {} if entry is None else {prefix: entry}
            with self.subTest(entry=entry), self.assertRaisesRegex(ValueError, "main-tree Perfetto Gitlink"):
                self.checked(inputs, catalog, checkout, lambda *_: self.fail("manifest must not be read"))

    def test_absent_or_different_actual_checkout_cannot_be_called_platform_coverage(self):
        for status in ("exact_manifest_entry_absent", "revision_mismatch", "repository_mismatch"):
            with self.subTest(status=status), self.assertRaisesRegex(ValueError, "every original manifest"):
                self.checked(*self.fixture(), lambda *_: {"status": status})

    def test_foreign_escaping_or_existing_output_refused_before_inputs(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-perfetto-") as name:
            root = Path(name)
            for output in (root / "foreign", root / "../perfetto-license-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, output)
            output = root / "perfetto-license-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_preflight_failure_does_not_create_output(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-perfetto-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: (_ for _ in ()).throw(ValueError("bad pin"))), \
                    self.assertRaisesRegex(ValueError, "bad pin"):
                SUBJECT["collect"](root, root / "perfetto-license-source-01")
            self.assertFalse((root / "perfetto-license-source-01").exists())

    def test_late_failure_keeps_partial_receipt_and_inert_child(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-perfetto-") as name:
            root = Path(name)
            def fail(_component, output, _references, pins):
                self.assertEqual(pins, SUBJECT["PINS"])
                (output / "partial.source-data").write_bytes(b"source evidence")
                raise ValueError("fixture fetch failure")
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main", "references": []}, "checkout", [])), \
                    patch.dict(SUBJECT["EXTERNAL"], collect_component=fail), self.assertRaisesRegex(ValueError, "fixture fetch"):
                SUBJECT["collect"](root, root / "perfetto-license-source-01")
            output = root / "perfetto-license-source-01"
            report = json.loads((output / "perfetto-source-inventory.json").read_text())
            self.assertEqual((output / "partial.source-data").read_bytes(), b"source evidence")
            self.assertFalse(report["collectionComplete"])
            self.assertEqual(report["failureType"], "ValueError")
            for key in ("sourceFunctionsExecuted", "generatorExecuted", "shippingDecisionsMade", "metadataPathsAliased", "promoted"):
                self.assertFalse(report[key])

    def test_successful_collection_preserves_real_root_and_no_shipping_claim(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-perfetto-") as name:
            root = Path(name)
            prefix, repository, revision = SUBJECT["PINS"]["perfetto"]
            component = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                         "treeEntries": 10, "selectedFiles": [{"path": "LICENSE"}], "sourceBytes": 24, "collectionComplete": True}
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main", "references": []}, "checkout", [])), \
                    patch.dict(SUBJECT["EXTERNAL"], collect_component=lambda *_a, **_k: component), \
                    patch.dict(SUBJECT["INPUTS"], receipt=lambda *_: (component, "child-seal")):
                report = SUBJECT["collect"](root, root / "perfetto-license-source-01")
            self.assertTrue(report["collectionComplete"])
            self.assertEqual(report["components"][0]["checkoutPath"], "third_party/perfetto")
            self.assertEqual(report["components"][0]["receiptSha256"], "child-seal")
            self.assertEqual(report["licenseCoverage"], "not_proven")
            self.assertFalse(report["metadataPathsAliased"])
            self.assertFalse(report["promoted"])


if __name__ == "__main__":
    unittest.main()
