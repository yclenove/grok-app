"""Fixed actual recursive pins, preserved failures, no shipping inference."""
import copy
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-checked-recursive-sources.py")))


class CheckedRecursiveTests(unittest.TestCase):
    def report(self):
        groups = {"angle": [], "openscreen": []}
        for key, (prefix, repository, revision) in SUBJECT["PINS"].items():
            groups[key.split("-", 1)[0]].append({
                "checkoutPath": "src/" + prefix, "expectedRepository": repository, "expectedRevision": revision,
                "actual": {"repository": repository, "revision": revision},
                "status": "exact_repository_and_revision_match", "conditionRecorded": None,
                "conditionEvaluated": False, "shippingDecisionMade": False})
        return {"sourceCommit": SUBJECT["INPUTS"]["TREE"]["COMMIT"], "conditionsEvaluated": False,
                "licenseErrorsExcluded": False, "promoted": False, "targets": [
                    {"target": key, "recursiveComparison": copy.deepcopy(groups)}
                    for key in SUBJECT["CHECKOUTS"]["BUILDS"]["ARCHIVES"]]}

    def test_exact_six_pins_require_all_four_archive_manifest_matches(self):
        self.assertEqual(len(SUBJECT["reviewed_pins"](self.report())), 6)
        self.assertTrue(all(url.startswith("https://chromium.googlesource.com/") and len(oid) == 40
                            for _, url, oid in SUBJECT["PINS"].values()))

    def test_missing_duplicate_or_foreign_target_not_only_first_platform_reject(self):
        for variant in ("missing", "duplicate", "foreign"):
            report = self.report()
            if variant == "missing":
                report["targets"].pop()
            else:
                report["targets"][-1]["target"] = report["targets"][0]["target"] if variant == "duplicate" else "other"
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["reviewed_pins"](report)

    def test_wrong_repository_revision_actual_or_path_cannot_be_aliased(self):
        for key, value in (("expectedRepository", "https://example.test/source"),
                           ("expectedRevision", "a" * 40), ("actual", None), ("checkoutPath", "src/other")):
            report = self.report()
            report["targets"][-1]["recursiveComparison"]["angle"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["reviewed_pins"](report)

    def test_absence_duplicate_extra_match_and_shipping_decision_never_ignored(self):
        for variant in ("absent", "duplicate", "extra", "shipping", "evaluated"):
            report = self.report()
            rows = report["targets"][-1]["recursiveComparison"]["openscreen"]
            if variant == "absent":
                rows.pop()
            elif variant == "duplicate":
                rows.append(copy.deepcopy(rows[0]))
            elif variant == "extra":
                rows.append({**rows[0], "checkoutPath": "src/seventh"})
            else:
                rows[0]["shippingDecisionMade" if variant == "shipping" else "conditionEvaluated"] = True
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["reviewed_pins"](report)

    def test_sealed_comparison_reproduced_before_loading_references(self):
        original = self.report()
        with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None,
                        receipt=lambda path, sha: (original, sha), load_inputs=lambda *args: {"references": ["original"]}), \
                patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda _: original):
            self.assertEqual(SUBJECT["checked_inputs"](Path("fixture"))[0], ["original"])
        with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None, receipt=lambda path, sha: (original, sha)), \
                patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda _: {}), self.assertRaises(ValueError):
            SUBJECT["checked_inputs"](Path("fixture"))

    def test_foreign_escaping_existing_output_reject_before_git(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "foreign")
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "../checked-recursive-license-01")
            output = root / "checked-recursive-license-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def fixture_component(self, key, output, references, pins):
        self.assertIs(pins, SUBJECT["PINS"])
        self.assertEqual(references, [{"sourcePath": "original/source"}])
        prefix, repository, revision = pins[key]
        result = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                  "treeEntries": 1, "selectedFiles": [], "sourceBytes": 1, "collectionComplete": True}
        directory = output / key
        directory.mkdir()
        SUBJECT["EXTERNAL"]["write_report"](directory / "license-source-inventory.json", result)
        return result

    def run_fixture(self, root, component):
        with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda _: ([{"sourcePath": "original/source"}], "fixture")), \
                patch.dict(SUBJECT["EXTERNAL"], collect_component=component):
            return SUBJECT["collect"](root, root / "checked-recursive-license-01")

    def test_success_retains_six_independent_receipt_seals_without_promoting(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-") as name:
            report = self.run_fixture(Path(name), self.fixture_component)
            self.assertTrue(report["collectionComplete"])
            self.assertEqual(len(report["components"]), 6)
            self.assertTrue(all(len(row["receiptSha256"]) == 64 for row in report["components"]))
            self.assertFalse(report["promoted"])
            self.assertFalse(report["shippingDecisionsMade"])

    def test_late_git_failure_retains_first_source_and_partial_root_receipt(self):
        def component(key, output, references, pins):
            if key != next(iter(pins)):
                raise RuntimeError("fixture terminal git failure")
            return self.fixture_component(key, output, references, pins)
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-") as name:
            root = Path(name)
            with self.assertRaises(RuntimeError):
                self.run_fixture(root, component)
            report = json.loads((root / "checked-recursive-license-01/checked-recursive-source-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertEqual(len(report["components"]), 1)
            self.assertFalse(report["promoted"])
            self.assertEqual(report["failureType"], "RuntimeError")


if __name__ == "__main__":
    unittest.main()
