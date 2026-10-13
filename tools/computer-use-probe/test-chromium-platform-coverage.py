"""Platform joins preserve raw errors, identities, casing and uncertainty."""
import copy
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-platform-coverage.py")))
COMMIT = SUBJECT["INPUTS"]["TREE"]["COMMIT"]


class PlatformCoverageTests(unittest.TestCase):
    def original_collection(self):
        groups = SUBJECT["EXTERNAL"]["GROUPS"]
        parents = [{"component": key, "revision": pin[2]} for name in ("core", "media", "runtime", "support", "browser")
            for key, pin in groups[name].items()]
        recursive = [{"component": key, "checkoutPath": path, "repository": repository, "revision": revision}
            for key, (path, repository, revision) in SUBJECT["RECURSIVE"]["RECURSIVE"]["PINS"].items()]
        return {"components": parents, "mainSha256": "1" * 64}, recursive

    def test_later_supplements_cannot_expand_original_57_source_scope(self):
        inputs, recursive = self.original_collection()
        before = copy.deepcopy(inputs)
        self.assertEqual(len(inputs["components"]), 57)
        added = {"future-notices": {"future": ("third_party/future", "https://example.test/future.git", "a" * 40)}}
        with patch.dict(SUBJECT["EXTERNAL"]["GROUPS"], added):
            sources = SUBJECT["collected_sources"](inputs, recursive)
        self.assertEqual(len(sources), 64)
        self.assertEqual(inputs, before)
        self.assertFalse({"webgl", "dawn", "googletest", "future"}.intersection(row["component"] for row in sources))

    def test_missing_duplicate_or_supplement_in_original_receipts_refuses(self):
        inputs, recursive = self.original_collection()
        original = inputs["components"]
        extra = {"component": "googletest", "revision": SUBJECT["EXTERNAL"]["DAWN_TEST_PINS"]["googletest"][2]}
        for rows in (original[:-1], original[:-1] + [original[0]], original[:-1] + [extra], original + [extra]):
            with self.assertRaises(ValueError):
                SUBJECT["collected_sources"]({**inputs, "components": rows}, recursive)

    def test_original_revision_and_recursive_root_uniqueness_remain_required(self):
        inputs, recursive = self.original_collection()
        inputs["components"][0]["revision"] = "0" * 40
        with self.assertRaises(ValueError):
            SUBJECT["collected_sources"](inputs, recursive)
        inputs, recursive = self.original_collection()
        for rows in (recursive[:-1], recursive[:-1] + [recursive[0]]):
            with self.assertRaises(ValueError):
                SUBJECT["collected_sources"](inputs, rows)

    def sources(self):
        return [{"component": "chromium", "checkoutPath": "src",
                 "repository": "https://chromium.googlesource.com/chromium/src.git", "revision": COMMIT},
                {"component": "parent", "checkoutPath": "src/third_party/parent",
                 "repository": "https://example.test/parent.git", "revision": "a" * 40},
                {"component": "child", "checkoutPath": "src/third_party/parent/third_party/child",
                 "repository": "https://example.test/child.git", "revision": "b" * 40}]

    def manifest(self):
        return {row["checkoutPath"]: {key: row[key] for key in ("repository", "revision")}
                for row in self.sources()}

    def view(self, manifest=None, declared=None, index=None, files=None):
        return SUBJECT["PlatformView"](self.sources(), self.manifest() if manifest is None else manifest,
                                       declared or [], index or {}, files or {})

    def test_longest_collected_and_actual_roots_keep_nested_revision(self):
        path = "third_party/parent/third_party/child/LICENSE"
        row = self.view(index={path: {"mode": "100644"}}, files={path: b"text"}).describe(path)
        self.assertEqual(row["collectedComponent"], "child")
        self.assertEqual(row["actualManifestIdentity"]["revision"], "b" * 40)
        self.assertEqual(row["collectedTreeManifestStatus"], "exact_repository_and_revision_match")
        self.assertEqual(row["pathStatus"], "resolved_to_verified_source_bytes")
        self.assertFalse(row["effectiveSourceMappingProven"])

    def test_manifest_absence_preserves_collected_child_and_no_shipping_exemption(self):
        manifest = self.manifest()
        del manifest[self.sources()[-1]["checkoutPath"]]
        row = self.view(manifest).describe("third_party/parent/third_party/child/LICENSE")
        self.assertEqual(row["collectedComponent"], "child")
        self.assertEqual(row["collectedTreeManifestStatus"], "exact_manifest_entry_absent")
        self.assertEqual(row["nearestActualManifestRoot"], "src/third_party/parent")
        self.assertEqual(row["pathStatus"], "path_not_in_collected_checkout_trees")
        self.assertFalse(row["shippingDecisionMade"])

    def test_different_revision_or_repository_suffix_is_not_aliased(self):
        for key, value in (("revision", "c" * 40), ("repository", "https://example.test/child")):
            manifest = self.manifest()
            manifest[self.sources()[-1]["checkoutPath"]][key] = value
            with self.subTest(key=key):
                row = self.view(manifest).describe("third_party/parent/third_party/child/LICENSE")
                self.assertEqual(row["collectedTreeManifestStatus"], "manifest_identity_differs")
                self.assertEqual(row["actualRootSourceStatus"], "manifest_identity_differs")

    def test_actual_uncollected_source_is_not_inferred_from_collected_parent(self):
        manifest = self.manifest()
        manifest["src/third_party/parent/third_party/other"] = {"repository": "https://example.test/other.git", "revision": "d" * 40}
        row = self.view(manifest).describe("third_party/parent/third_party/other/LICENSE")
        self.assertEqual(row["collectedComponent"], "parent")
        self.assertEqual(row["actualRootSourceStatus"], "actual_manifest_source_not_collected")
        actual = self.view(manifest).manifest_sources()
        self.assertEqual(sum(row["selectedSourceStatus"] == "actual_manifest_source_not_collected" for row in actual), 1)

    def test_non_git_generated_input_and_actual_manifest_are_independent(self):
        declared = [{"path": "third_party/node/node_modules", "kind": "non_git", "nonGitKind": "cipd",
                     "conditionRecorded": "checkout_linux", "conditionEvaluated": False}]
        row = self.view(declared=declared).describe("third_party/node/node_modules/lit/LICENSE")
        self.assertEqual(row["nearestDeclaredSource"], declared[0])
        self.assertEqual(row["nearestActualManifestRoot"], "src")
        self.assertEqual(row["pathStatus"], "path_not_in_collected_checkout_trees")
        self.assertFalse(row["nearestDeclaredSource"]["conditionEvaluated"])

    def test_uncollected_gitlink_remains_visible_not_substituted(self):
        root = "third_party/parent/third_party/other"
        row = self.view(index={root: {"mode": "160000", "gitObjectSha1": "e" * 40}}).describe(root + "/LICENSE")
        self.assertEqual(row["nearestIndexedGitlink"], {"path": root, "revision": "e" * 40, "selectedSourceCollected": False})
        self.assertEqual(row["collectedComponent"], "parent")

    def test_exact_file_modes_do_not_confuse_uncollected_bytes_or_symlinks(self):
        for mode, expected in (("100644", "regular_source_exists_but_bytes_not_collected"),
                               ("120000", "reference_is_not_regular_source_file")):
            with self.subTest(mode=mode):
                row = self.view(index={"LICENSE": {"mode": mode}}).describe("LICENSE")
                self.assertEqual(row["pathStatus"], expected)
                self.assertEqual(row["indexedMode"], mode)

    def test_remote_reference_has_no_fabricated_owner_or_network(self):
        self.assertEqual(self.view().describe(None), {"path": None, "pathStatus": "no_local_source_path", "shippingDecisionMade": False})

    def test_invalid_paths_duplicate_roots_or_manifest_identity_reject(self):
        for path in ("../LICENSE", "third_party//LICENSE", "third_party\\LICENSE", "C:/LICENSE"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.view().describe(path)
        with self.assertRaises(ValueError):
            SUBJECT["PlatformView"](self.sources() + [self.sources()[-1]], self.manifest(), [], {}, {})
        manifest = self.manifest()
        manifest["src"]["revision"] = "a" * 40
        with self.assertRaises(ValueError):
            self.view(manifest)

    def test_all_original_errors_and_case_sensitive_values_preserved_per_platform(self):
        references = [{"metadata": "third_party/parent/README.chromium", "sourcePath": None,
                       "value": "https://example.test/LICENSE", "resolution": "remote_reference_not_followed"}]
        entry = {"directory": "third_party/parent", "shippedMetadata": "yes", "errors": ["original error"],
                 "declaredLicensePaths": ["third_party/parent/LICENSE"], "availableLicensePaths": []}
        scan = {"entries": [entry, {**entry, "shippedMetadata": "Yes"}, {**entry, "shippedMetadata": "No"}],
                "directoryErrors": [{"directory": "third_party/parent/third_party/other", "error": "missing metadata"}]}
        originals = copy.deepcopy((references, scan))
        result = SUBJECT["platform_report"]({"target": "fixture", "buildId": "123", "manifestEntries": 3},
                                            self.view(), references, scan, {"YES": "yes", "NO": "no"})
        self.assertEqual((references, scan), originals)
        self.assertEqual(len(result["shippingMetadataEntriesWithErrors"]), 1)
        self.assertEqual([row["shippedMetadata"] for row in result["nonstandardShippingValues"]], ["Yes", "No"])
        self.assertEqual(result["unresolvedRawReferences"][0]["value"], references[0]["value"])
        self.assertEqual(result["directoryErrors"][0]["error"], "missing metadata")
        self.assertTrue(result["originalErrorsPreserved"])
        self.assertFalse(result["promoted"])
        self.assertEqual(result["licenseCoverage"], "not_proven")

    def test_output_path_escape_and_existing_file_reject_before_source_load(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-platform-coverage-") as name:
            root = Path(name)
            for path in (root / "other.json", root / "../platform-source-coverage-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](root, path)
            path = root / "platform-source-coverage-01.json"
            path.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, path)
            self.assertEqual(path.read_bytes(), b"original")

    def test_incomplete_collected_source_inventory_rejects(self):
        with self.assertRaisesRegex(ValueError, "57 original"):
            SUBJECT["collected_sources"]({"components": []}, [])

    def test_absent_recursive_declaration_is_not_a_nonshipping_decision(self):
        declared = [{"path": "third_party/parent/third_party/missing", "kind": "git",
                     "repository": "https://example.test/missing", "revision": "c" * 40}]
        row = self.view(declared=declared).describe(declared[0]["path"] + "/LICENSE")
        self.assertEqual(row["nearestDeclaredSourceManifestStatus"], "exact_manifest_entry_absent")
        self.assertEqual(SUBJECT["gap_category"]({"value": "LICENSE"}, row), "declared_git_root_absent_from_actual_manifest")
        self.assertFalse(row["shippingDecisionMade"])

    def test_not_included_and_non_git_gap_categories_do_not_close_reference(self):
        declared = [{"path": "third_party/node/node_modules", "kind": "non_git"}]
        row = self.view(declared=declared).describe("third_party/node/node_modules/lit/LICENSE")
        self.assertEqual(SUBJECT["gap_category"]({"value": "LICENSE"}, row), "declared_non_git_input_not_collected")
        self.assertEqual(SUBJECT["gap_category"]({"value": "Not Included"}, row), "nonstandard_not_included_not_an_exemption")
        self.assertEqual(row["pathStatus"], "path_not_in_collected_checkout_trees")

    def test_nearby_prefix_is_not_a_source_owner_match(self):
        row = self.view().describe("third_party/parent_not_child/LICENSE")
        self.assertEqual(row["collectedComponent"], "chromium")
        self.assertEqual(row["nearestActualManifestRoot"], "src")


if __name__ == "__main__":
    unittest.main()
