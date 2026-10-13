"""Independent recursive seals and no exclusions or false shipping claims."""
import copy
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-checked-recursive-licenses.py")))


class RecursiveLicenseTests(unittest.TestCase):
    def root(self):
        return {"sourceCommit": SUBJECT["INPUTS"]["TREE"]["COMMIT"],
                "checkoutReportSha256": SUBJECT["RECURSIVE"]["CHECKOUT_REPORT_SHA"],
                "requestedComponents": list(SUBJECT["RECURSIVE"]["PINS"]), "collectionComplete": True,
                "sourceFunctionsExecuted": False, "conditionsEvaluated": False,
                "shippingDecisionsMade": False, "generatorExecuted": False, "promoted": False,
                "components": [{"component": key, "receiptSha256": sha} for key, sha in SUBJECT["SEALS"].items()]}

    def test_complete_root_six_independent_seals_and_original_checkout_required(self):
        SUBJECT["checked_root"](self.root())
        self.assertEqual(set(SUBJECT["SEALS"]), set(SUBJECT["RECURSIVE"]["PINS"]))

    def test_missing_reordered_duplicate_components_or_modified_seal_reject(self):
        for variant in ("missing", "order", "duplicate", "seal"):
            report = self.root()
            if variant == "missing":
                report["components"].pop()
            elif variant == "order":
                report["components"].reverse()
            elif variant == "duplicate":
                report["components"][-1] = copy.deepcopy(report["components"][0])
            else:
                report["components"][-1]["receiptSha256"] = "a" * 64
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["checked_root"](report)

    def test_promotion_execution_shipping_or_foreign_checkout_flags_reject(self):
        for key, value in (("sourceFunctionsExecuted", True), ("conditionsEvaluated", 0),
                           ("shippingDecisionsMade", True), ("generatorExecuted", True), ("promoted", True),
                           ("checkoutReportSha256", "a" * 64), ("sourceCommit", "b" * 40)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["checked_root"]({**self.root(), key: value})

    def test_changed_component_identity_reject_before_tree_read(self):
        root = self.root()
        component = {"checkoutPath": "other", "repository": "https://example.test/other", "revision": "a" * 40}
        with patch.dict(SUBJECT["INPUTS"], receipt=lambda path, sha: (root if path.name.startswith("checked-recursive") else component, sha)), \
                self.assertRaisesRegex(ValueError, "recursive source identity"):
            SUBJECT["append_sources"](Path("fixture"), {})

    def test_foreign_escaping_existing_output_reject_without_input_load(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-audit-") as name:
            root = Path(name)
            for output in (root / "foreign", root / "../checked-recursive-license-audit-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](root, output)
            output = root / "checked-recursive-license-audit-01.json"
            output.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"original")

    def test_partial_recursive_collection_never_excludes_absent_or_promotes(self):
        old_gap = {"metadata": "original", "sourcePath": "new/license", "value": "LICENSE"}
        old = {"unresolvedRawReferences": [old_gap]}
        new = {"unresolvedRawReferences": [{"sourcePath": "still/missing"}]}
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-audit-") as name:
            root = Path(name)
            inputs = {"mainSha256": "fixture", "components": ["original"], "index": {}, "files": {}}
            calls = []
            def summary(_):
                calls.append(1)
                return old if len(calls) == 1 else new
            with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *args: inputs,
                            classify=lambda *args: "resolved_to_verified_source_bytes"), \
                    patch.dict(SUBJECT["inspect"].__globals__, summarize=summary, append_sources=lambda *_: (["recursive"], "fixture")):
                report = SUBJECT["inspect"](root, root / "checked-recursive-license-audit-01.json")
            self.assertEqual(report["closedOriginalRawReferences"], [old_gap])
            self.assertEqual(len(report["after"]["unresolvedRawReferences"]), 1)
            for key in ("originalAbsentSourcesExcluded", "allRecursiveSourcesCollected", "conditionsEvaluated", "promoted"):
                self.assertFalse(report[key])
            self.assertEqual(report["licenseCoverage"], "not_proven")


if __name__ == "__main__":
    unittest.main()
