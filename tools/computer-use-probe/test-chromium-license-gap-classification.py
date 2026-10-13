"""Ownership classification never licenses a binary or resolves conditions."""
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-license-gap-classification.py")))


class ClassificationTests(unittest.TestCase):
    def catalog(self):
        return {"gitDependencies": [
            {"path": "third_party/outer", "condition": None, "revision": "a" * 40,
             "repository": "https://example.org/outer.git"},
            {"path": "third_party/linux/src", "condition": "checkout_linux", "revision": "b" * 40,
             "repository": "https://example.org/linux.git"}],
            "nonGitDependencies": [{"path": "third_party/generated/assets", "kind": "gcs",
                                    "condition": "checkout_android and non_git_source"}]}

    def index(self):
        return {"LICENSE": {"mode": "100644"}, "missing-bytes": {"mode": "100755"},
                "link": {"mode": "120000"},
                "third_party/outer": {"mode": "160000", "gitObjectSha1": "a" * 40},
                "third_party/outer/nested/src": {"mode": "160000", "gitObjectSha1": "c" * 40},
                "third_party/linux/src": {"mode": "160000", "gitObjectSha1": "b" * 40}}

    def owners(self):
        return SUBJECT["SourceOwners"](self.catalog(), self.index(), {"third_party/outer"})

    def classify(self, path, **extra):
        return SUBJECT["classify_reference"]({"sourcePath": path, **extra}, self.index(), {"LICENSE": {}}, self.owners())

    def test_collected_main_and_parent_owners_are_not_complete_checkout_or_usage_claims(self):
        owners = self.owners()
        for path, expected in (("LICENSE", "collected_main_tree"),
                               ("third_party/outer/LICENSE", "collected_parent_git_source")):
            actual = owners.describe(path)
            self.assertEqual(actual["originKind"], expected)
            self.assertFalse(actual["declaredConditionsEvaluated"])
            self.assertFalse(actual["binaryUsageProven"])

    def test_uncollected_linux_declared_condition_preserved_but_never_evaluated(self):
        actual = self.classify("third_party/linux/src/LICENSE")
        self.assertEqual(actual["category"], "uncollected_parent_git_source")
        self.assertEqual(actual["sourceOwner"]["parentDeclaration"]["condition"], "checkout_linux")
        self.assertFalse(actual["sourceOwner"]["declaredConditionsEvaluated"])

    def test_recursive_gitlink_different_from_collected_parent_preserves_exact_oid(self):
        actual = self.classify("third_party/outer/nested/src/LICENSE")
        self.assertEqual(actual["category"], "uncollected_recursive_gitlink")
        self.assertEqual(actual["sourceOwner"]["pendingGitlink"],
                         {"path": "third_party/outer/nested/src", "revision": "c" * 40})
        self.assertEqual(actual["sourceOwner"]["parentDeclaration"]["path"], "third_party/outer")

    def test_more_specific_declared_source_is_not_misclassified_by_ancestor_stub(self):
        catalog = self.catalog()
        catalog["gitDependencies"].append({"path": "third_party/outer/nested/src", "condition": None,
                                           "revision": "c" * 40, "repository": "https://example.org/nested.git"})
        owners = SUBJECT["SourceOwners"](catalog, self.index(), {"third_party/outer/nested/src"})
        self.assertEqual(owners.describe("third_party/outer/nested/src/LICENSE")["originKind"],
                         "collected_parent_git_source")
        self.assertIsNone(owners.describe("third_party/outer/nested/src/LICENSE")["pendingGitlink"])

    def test_non_git_generated_input_stays_explicit_not_excluded_for_android_condition(self):
        actual = self.classify("third_party/generated/assets/LICENSE")
        self.assertEqual(actual["category"], "declared_non_git_source")
        declaration = actual["sourceOwner"]["parentDeclaration"]
        self.assertEqual(declaration["nonGitKind"], "gcs")
        self.assertEqual(declaration["condition"], "checkout_android and non_git_source")

    def test_boundary_match_does_not_alias_similar_sibling_names(self):
        actual = self.classify("third_party/linux/src-other/LICENSE")
        self.assertEqual(actual["category"], "absent_in_collected_source_trees")
        self.assertIsNone(actual["sourceOwner"]["parentDeclaration"])

    def test_known_source_regular_file_without_bytes_is_not_an_external_gap(self):
        actual = self.classify("missing-bytes")
        self.assertEqual(actual["category"], "regular_source_exists_but_bytes_not_collected")

    def test_verified_bytes_and_nonregular_source_are_not_guessed_from_owner(self):
        self.assertEqual(self.classify("LICENSE")["category"], "resolved_to_verified_source_bytes")
        for path in ("link", "third_party/linux/src"):
            self.assertEqual(self.classify(path)["category"], "reference_is_not_regular_source_file")

    def test_remote_reference_is_data_no_path_or_request(self):
        actual = self.classify(None, value="https://example.org/LICENSE")
        self.assertEqual(actual["category"], "remote_reference_not_followed")
        self.assertIsNone(actual["sourceOwner"])

    def test_invalid_declared_license_path_is_not_mislabeled_as_remote_reference(self):
        actual = self.classify(None, value="../../../../secret", reason="unsupported_or_outside_root")
        self.assertEqual(actual["category"], "unsupported_or_outside_root_license_reference")
        actual = self.classify(None, value="https://example.org/LICENSE", reason="unsupported_or_outside_root")
        self.assertEqual(actual["category"], "remote_reference_not_followed")

    def test_not_included_marker_keeps_original_missing_ref_never_becomes_not_shipped(self):
        actual = self.classify("third_party/outer/Not Included", value="Not Included")
        self.assertEqual(actual["category"], "nonstandard_not_included_license_value")
        self.assertEqual(actual["resolution"], "path_not_in_collected_checkout_trees")
        self.assertEqual(actual["sourcePath"], "third_party/outer/Not Included")
        self.assertFalse(actual["sourceOwner"]["binaryUsageProven"])
        self.assertEqual(self.classify("LICENSE", value="Not Included")["category"], "resolved_to_verified_source_bytes")

    def test_duplicate_cross_kind_owner_and_unsafe_paths_reject(self):
        catalog = self.catalog()
        catalog["nonGitDependencies"][0]["path"] = "third_party/outer"
        with self.assertRaises(ValueError):
            SUBJECT["SourceOwners"](catalog, {}, set())
        for path in ("", "../escape", "/root", "C:/secret", "x\\y", "x\0y", "x//y"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                SUBJECT["canonical_path"](path)

    def test_fresh_output_and_foreign_source_reject_without_following_any_network_reference(self):
        for name in ("../escape", "license-gap-classification-bad.json"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["inspect"](Path("foreign"), Path("foreign") / name)
        with self.assertRaises(ValueError):
            SUBJECT["inspect"](Path("foreign"), Path("foreign/license-gap-classification-01.json"))
        with tempfile.TemporaryDirectory(prefix="grok-cu-gap-classification-") as name:
            root = Path(name)
            output = root / "license-gap-classification-01.json"
            output.write_bytes(b"old evidence")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"old evidence")

    def test_unknown_shipped_values_not_case_normalized_and_directory_errors_not_dropped(self):
        entry = {"directory": "third_party/linux", "shippedMetadata": "Yes", "errors": [],
                 "metadata": "README.chromium"}
        scan = {"entries": [entry], "directoryErrors": [
            {"directory": "third_party/tests", "metadata": None, "error": "missing metadata"}],
            "candidateDirectories": ["third_party/linux", "third_party/tests"]}
        inputs = {"files": {"tools/licenses/licenses.py": {"payload": b"verified policy data"}},
                  "index": {}, "references": [], "components": []}
        with patch.dict(SUBJECT["POLICY"], parse_policy=lambda _: {"YES": "yes", "NO": "no", "VCS_METADATA_DIRS": ()},
                        scan=lambda *_: scan):
            report = SUBJECT["classify"](inputs, self.catalog())
        self.assertEqual(report["nonstandardShippingValues"][0]["shippedMetadata"], "Yes")
        self.assertFalse(report["nonstandardShippingValues"][0]["caseNormalized"])
        self.assertEqual(report["nonstandardShippingValues"][0]["shippingDecision"], "not_made")
        self.assertEqual(len(report["directoryErrors"]), 1)
        self.assertEqual(report["candidateDirectories"], 2)

    def test_error_entries_preserve_all_declared_missing_references_and_metadata_errors(self):
        entry = {"directory": "third_party/linux", "shippedMetadata": "yes", "errors": ["missing field"],
                 "metadata": "README.chromium", "unresolvedDeclaredReferences": [
                     {"path": "third_party/linux/src/LICENSE", "value": "src/LICENSE", "reason": "license_bytes_not_collected"}]}
        scan = {"entries": [entry], "directoryErrors": [], "candidateDirectories": ["third_party/linux"]}
        inputs = {"files": {"tools/licenses/licenses.py": {"payload": b"verified policy data"}},
                  "index": self.index(), "references": [], "components": []}
        with patch.dict(SUBJECT["POLICY"], parse_policy=lambda _: {"YES": "yes", "NO": "no", "VCS_METADATA_DIRS": ()},
                        scan=lambda *_: scan):
            report = SUBJECT["classify"](inputs, self.catalog())
        actual = report["shippingMetadataEntriesWithErrors"][0]
        self.assertEqual(actual["errors"], ["missing field"])
        self.assertEqual(actual["classifiedDeclaredGaps"][0]["category"], "uncollected_parent_git_source")


if __name__ == "__main__":
    unittest.main()
