"""Combined notice exports retain errors and reject partial or altered evidence."""
from collections import Counter
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("assemble-chromium-notice-candidate.py")))


class NoticeAssemblyTests(unittest.TestCase):
    def support(self):
        expected = {"dawn_googletest_dir": ("//third_party/googletest/src", "third_party/googletest/src/LICENSE"),
            "dawn_partition_alloc_dir": ("//base/allocator/partition_allocator", "LICENSE")}
        dawn = {"after": {"originalErrors": ["unchanged"]}, "configuredSourceLicenseBindings": [{"original": True}],
            "configurationEvidence": {"declaredDirectories": {key: pair[0] for key, pair in expected.items()}}}
        support = {"originalDawnErrorsRetained": copy.deepcopy(dawn["after"]),
            "configurationEvidence": copy.deepcopy(dawn["configurationEvidence"]),
            "originalConfiguredBindingsRetained": copy.deepcopy(dawn["configuredSourceLicenseBindings"]),
            "configuredDirectoryBindings": 17, "newConfiguredSourceLicenseBindings": [
                {"variable": key, "declaredDirectory": directory, "license": {"sourcePath": path},
                    "completeComponentNoticeCovered": False, "binaryUseProven": False} for key, (directory, path) in expected.items()]}
        return support, dawn

    def test_current_candidate_requires_and_preserves_all_eight_supplements(self):
        self.assertEqual(len(SUBJECT["CURRENT_SUPPLEMENTS"]), 8)
        fragments = {key: ("original notice " + key).encode() for key in SUBJECT["CURRENT_SUPPLEMENTS"]}
        result = SUBJECT["render_candidate"](b"base", fragments, SUBJECT["CURRENT_SUPPLEMENTS"])
        for data in fragments.values():
            self.assertIn(data, result)
        with self.assertRaises(ValueError):
            SUBJECT["render_candidate"](b"base", self.fragments(), SUBJECT["CURRENT_SUPPLEMENTS"])
        with self.assertRaises(ValueError):
            SUBJECT["render_candidate"](b"base", fragments, {**SUBJECT["CURRENT_SUPPLEMENTS"]})

    def test_original_six_supplement_scope_can_still_be_reproduced_explicitly(self):
        previous = SUBJECT["DAWN_SUPPORT_SUPPLEMENTS"]
        self.assertEqual(len(previous), 6)
        fragments = {key: ("original " + key).encode() for key in previous}
        result = SUBJECT["render_candidate"](b"base", fragments, previous)
        self.assertTrue(all(data in result for data in fragments.values()))
        self.assertNotIn(b"registry-xml", result)

    def test_original_seven_supplement_scope_remains_frozen(self):
        previous = SUBJECT["REGISTRY_SUPPLEMENTS"]
        self.assertEqual(len(previous), 7)
        fragments = {key: ("original " + key).encode() for key in previous}
        result = SUBJECT["render_candidate"](b"base", fragments, previous)
        self.assertTrue(all(data in result for data in fragments.values()))
        self.assertNotIn(b"angle-libraries", result)

    def libraries(self):
        current, api, dawn = self.dispositions()
        original = SUBJECT["metadata_dispositions"](current, api, dawn)
        unresolved = [row for row in original if row["disposition"] == "unresolved_source_or_shipping_obligation"]
        bindings = []
        for previous, (directory, (relative, sha)) in zip(unresolved, SUBJECT["ANGLE_LIBRARY_BINDINGS"].items()):
            previous["originalOmittedMetadata"].update(directory=directory, declaredLicensePaths=[directory + "/" + relative])
            bindings.append({"originalOmittedMetadata": copy.deepcopy(previous["originalOmittedMetadata"]),
                "sourceReceiptSha256": sha, "license": {"sourcePath": directory + "/" + relative, "role": "declared_library_license"},
                "completeComponentNoticeCovered": False, "binaryUseProven": False, "binaryExclusionProven": False})
        report = {"originalMetadataErrorsRetained": copy.deepcopy(original), "sourceLicenseBindings": bindings,
            "originalAssemblyReceiptSha256": "e9038e3ee26ebcecea2098feb6e85360a9012d399ee736a21a66b523358d03b1",
            "remainingOriginalSourceMetadata": [copy.deepcopy(unresolved[-1])]}
        return original, report

    def test_three_library_bindings_keep_raw_errors_and_only_resolve_three_source_gaps(self):
        original, libraries = self.libraries()
        unchanged = copy.deepcopy((original, libraries))
        result = SUBJECT["angle_library_dispositions"](original, libraries)
        self.assertEqual(Counter(row["disposition"] for row in result), {"configured_source_license_bound": 11,
            "copied_header_notices_bound_registry_checkout_not_proven": 3, "declared_source_license_bound": 3,
            "unresolved_source_or_shipping_obligation": 1})
        self.assertTrue(all(row["originalOmittedMetadata"]["errors"] and row["originalErrorRemoved"] is False for row in result))
        self.assertEqual((original, libraries), unchanged)

    def test_library_binding_duplicate_wrong_path_receipt_or_shipping_claim_refuses(self):
        for change in ("duplicate", "path", "receipt", "role", "binary", "coverage"):
            original, libraries = self.libraries()
            row = libraries["sourceLicenseBindings"][0]
            if change == "duplicate":
                libraries["sourceLicenseBindings"][1] = copy.deepcopy(row)
            elif change in ("path", "role"):
                row["license"]["sourcePath" if change == "path" else "role"] = "test_image_license"
            elif change == "receipt":
                row["sourceReceiptSha256"] = "foreign-source"
            else:
                row["binaryUseProven" if change == "binary" else "completeComponentNoticeCovered"] = True
            with self.subTest(change=change), self.assertRaises(ValueError):
                SUBJECT["angle_library_dispositions"](original, libraries)

    def test_library_report_cannot_drop_original_errors_remaining_sdk_or_source_context(self):
        for key, value in (("originalMetadataErrorsRetained", []), ("remainingOriginalSourceMetadata", []),
                ("originalAssemblyReceiptSha256", "another-candidate")):
            original, libraries = self.libraries()
            libraries[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["angle_library_dispositions"](original, libraries)

    def registry(self):
        api = {"headerSourceReceiptSha256": "header-seal", "selectedHeaders": [{"sourcePath": "header", "sourceSha256": "header-sha"}]}
        registry = {"originalRawErrorsRetained": {"originalErrors": ["unchanged"]},
            "copiedHeaderSourceReceiptSha256": "header-seal", "registryCollectionReceiptSha256": "registry-seal",
            "generationReceiptSha256": "generation-seal",
            "xmlNotices": [{"sourcePath": path} for path in sorted(SUBJECT["REGISTRY_XML_PATHS"])],
            "xmlGenerationMatches": [{"sourcePath": path, "matchesRecordedGenerationInput": True} for path in sorted(SUBJECT["REGISTRY_XML_PATHS"])],
            "copiedHeaderComparisons": [{"copiedSourcePath": "header", "sourceSha256": "header-sha", "status": "unmatched"}],
            "copiedHeaderComparisonCounts": {"unmatched": 1}}
        for key in ("completeHeaderProvenance", "releaseReady", "completeChromiumNotice", "binarySbom", "promoted",
                "originalErrorsRemoved", "shippingExclusionsProven"):
            registry[key] = False
        return registry, api, {"after": {"originalErrors": ["unchanged"]}}

    def test_xml_source_evidence_keeps_unmatched_headers_and_original_errors(self):
        registry, api, dawn = self.registry()
        original = copy.deepcopy((registry, api, dawn))
        result = SUBJECT["registry_evidence"](registry, api, dawn)
        self.assertEqual(result["copiedHeaderComparisonCounts"], {"unmatched": 1})
        self.assertFalse(result["completeHeaderProvenance"])
        self.assertFalse(result["binaryUseProven"])
        self.assertEqual((registry, api, dawn), original)

    def test_xml_missing_duplicate_mismatched_header_or_release_claim_refuses(self):
        for change in ("missing", "duplicate", "generation", "header", "errors", "release", "counts"):
            registry, api, dawn = self.registry()
            if change == "missing":
                registry["xmlNotices"].pop()
            elif change == "duplicate":
                registry["xmlNotices"][1] = registry["xmlNotices"][0]
            elif change == "generation":
                registry["xmlGenerationMatches"][0]["matchesRecordedGenerationInput"] = False
            elif change == "header":
                registry["copiedHeaderComparisons"][0]["sourceSha256"] = "other"
            elif change == "errors":
                registry["originalRawErrorsRetained"] = {}
            elif change == "release":
                registry["releaseReady"] = True
            else:
                registry["copiedHeaderComparisonCounts"] = {}
            with self.subTest(change=change), self.assertRaises(ValueError):
                SUBJECT["registry_evidence"](registry, api, dawn)

    def test_source_support_bindings_preserve_all_original_dawn_errors(self):
        support, dawn = self.support()
        actual = SUBJECT["support_bindings"](support, dawn)
        self.assertEqual(actual, support["newConfiguredSourceLicenseBindings"])
        self.assertTrue(all(row["binaryUseProven"] is False for row in actual))
        self.assertEqual(dawn["after"], {"originalErrors": ["unchanged"]})

    def test_missing_duplicate_redirected_or_complete_claim_support_refuses(self):
        for change in ("missing", "duplicate", "license", "directory", "binary", "complete"):
            support, dawn = self.support()
            rows = support["newConfiguredSourceLicenseBindings"]
            if change == "missing":
                rows.pop()
            elif change == "duplicate":
                rows[1] = copy.deepcopy(rows[0])
            elif change == "license":
                rows[0]["license"]["sourcePath"] = "third_party/dawn/third_party/googletest/LICENSE"
            elif change == "directory":
                rows[0]["declaredDirectory"] = "//other"
            elif change == "binary":
                rows[0]["binaryUseProven"] = True
            else:
                rows[0]["completeComponentNoticeCovered"] = True
            with self.subTest(change=change), self.assertRaises(ValueError):
                SUBJECT["support_bindings"](support, dawn)

    def test_support_cannot_remove_original_errors_configuration_or_binding_evidence(self):
        for key, value in (("originalDawnErrorsRetained", {}), ("configurationEvidence", {}),
                ("originalConfiguredBindingsRetained", []), ("configuredDirectoryBindings", 16)):
            support, dawn = self.support()
            support[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["support_bindings"](support, dawn)

    def fragments(self):
        return {key: ("original complete notice for " + key + "\n").encode() for key in SUBJECT["SUPPLEMENTS"]}

    def evidence(self, root, data=b"original copyright\ncomplete permission\ncomplete disclaimer\n"):
        report = {"sourceCommit": SUBJECT["INPUTS"]["TREE"]["COMMIT"],
            **{key: False for key in SUBJECT["SOURCE_ONLY_FLAGS"]}, "fragmentFile": "notice.txt",
            "fragmentBytes": len(data), "fragmentSha256": hashlib.sha256(data).hexdigest()}
        (root / "fragment").mkdir()
        (root / "fragment/notice.txt").write_bytes(data)
        return report, ("fragment", "inventory.json", "1" * 64, "notice.txt")

    def dispositions(self):
        kinds = (["configured_source_license_bound"] * 10
            + ["copied_header_notices_bound_registry_checkout_not_proven"] * 3
            + ["unresolved_source_or_shipping_obligation"] * 4)
        original = [{"originalOmittedMetadata": {"directory": "third_party/old-" + str(index),
            "errors": ["original missing permission"], "shippedMetadata": "yes"}, "disposition": kind,
            "originalErrorRemoved": False, "binaryExclusionProven": False} for index, kind in enumerate(kinds)]
        current = [copy.deepcopy(row["originalOmittedMetadata"]) for row in original]
        current.append({"directory": SUBJECT["DAWN_LOADER"], "errors": ["original missing permission"], "shippedMetadata": "yes"})
        dawn = {"configuredSourceLicenseBindings": [{"variable": "dawn_vulkan_loader_dir",
            "declaredDirectory": "//third_party/vulkan-loader/src", "license": {"sourcePath": "third_party/vulkan-loader/src/LICENSE.txt"}}],
            "configurationEvidence": {"declaredDirectories": {"dawn_vulkan_loader_dir": "//third_party/vulkan-loader/src"}}}
        return current, {"omittedMetadataDispositions": original}, dawn

    def test_all_five_complete_supplements_and_base_notice_survive(self):
        fragments = self.fragments()
        result = SUBJECT["render_candidate"](b"complete base permission\n", fragments)
        self.assertIn(b"complete base permission\n", result)
        self.assertTrue(result.startswith(b"MAINTAINED BROWSER SOURCE NOTICE CANDIDATE - NOT FOR DISTRIBUTION"))
        for data in fragments.values():
            self.assertIn(data, result)

    def test_missing_extra_empty_or_text_supplement_refuses(self):
        original = self.fragments()
        key = next(iter(original))
        for changed in ({name: data for name, data in original.items() if name != key},
                {**original, "foreign": b"other"}, {**original, key: b""}, {**original, key: "not byte evidence"}):
            with self.assertRaises(ValueError):
                SUBJECT["render_candidate"](b"base", changed)

    def test_total_budget_includes_headers_and_final_separators(self):
        result = SUBJECT["render_candidate"](b"base", self.fragments())
        with patch.dict(SUBJECT["render_candidate"].__globals__, MAX_NOTICE=len(result)):
            self.assertEqual(SUBJECT["render_candidate"](b"base", self.fragments()), result)
        with patch.dict(SUBJECT["render_candidate"].__globals__, MAX_NOTICE=len(result) - 1), self.assertRaises(ValueError):
            SUBJECT["render_candidate"](b"base", self.fragments())

    def test_partial_scope_markers_inside_original_fragments_are_preserved(self):
        fragments = self.fragments()
        fragments["copied-api"] = b"PARTIAL SOURCE NOTICE\nNOT FOR DISTRIBUTION\noriginal attribution\n"
        self.assertIn(fragments["copied-api"], SUBJECT["render_candidate"](b"base", fragments))

    def test_unchanged_sealed_fragment_returns_exact_original_bytes(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-notice-assembly-") as name:
            root = Path(name)
            report, spec = self.evidence(root)
            with patch.dict(SUBJECT["INPUTS"], receipt=lambda path, seal: (report, seal)):
                actual, data, evidence = SUBJECT["checked_fragment"](root, spec)
            self.assertEqual(actual, report)
            self.assertIn(b"complete disclaimer\n", data)
            self.assertEqual(evidence["receiptSha256"], "1" * 64)

    def test_changed_shortened_or_empty_fragment_refuses(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-notice-corrupt-") as name:
            root = Path(name)
            report, spec = self.evidence(root)
            for data in (b"different copyright", b"complete permission only", b""):
                (root / "fragment/notice.txt").write_bytes(data)
                with patch.dict(SUBJECT["INPUTS"], receipt=lambda path, seal: (report, seal)), self.assertRaises(ValueError):
                    SUBJECT["checked_fragment"](root, spec)

    def test_wrong_commit_promoted_scope_or_redirected_fragment_refuses(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-notice-scope-") as name:
            root = Path(name)
            original, spec = self.evidence(root)
            for key, value in ([("sourceCommit", "0" * 40), ("fragmentFile", "../outside")]
                    + [(key, True) for key in SUBJECT["SOURCE_ONLY_FLAGS"]]):
                report = {**original, key: value}
                with patch.dict(SUBJECT["INPUTS"], receipt=lambda path, seal: (report, seal)), self.assertRaises(ValueError):
                    SUBJECT["checked_fragment"](root, spec)

    def test_eighteen_raw_errors_are_classified_without_being_removed(self):
        rows = SUBJECT["metadata_dispositions"](*self.dispositions())
        self.assertEqual(Counter(row["disposition"] for row in rows), {"configured_source_license_bound": 11,
            "copied_header_notices_bound_registry_checkout_not_proven": 3, "unresolved_source_or_shipping_obligation": 4})
        self.assertEqual(len(rows), 18)
        self.assertTrue(all(row["originalOmittedMetadata"]["errors"] for row in rows))

    def test_changed_raw_error_or_shipping_value_refuses(self):
        for key, value in (("errors", []), ("shippedMetadata", "no")):
            current, api, dawn = self.dispositions()
            current[0][key] = value
            with self.assertRaises(ValueError):
                SUBJECT["metadata_dispositions"](current, api, dawn)

    def test_missing_duplicate_or_unrecognized_metadata_refuses(self):
        current, api, dawn = self.dispositions()
        for changed in (current[:-1], current[:-1] + [current[0]], current[:-1] + [{"directory": "foreign"}]):
            with self.assertRaises(ValueError):
                SUBJECT["metadata_dispositions"](changed, api, dawn)

    def test_dawn_redirected_license_or_ambiguous_binding_refuses(self):
        for change in ("license", "directory", "duplicate", "configuration"):
            current, api, dawn = self.dispositions()
            binding = dawn["configuredSourceLicenseBindings"][0]
            if change == "license":
                binding["license"]["sourcePath"] = "third_party/dawn/third_party/vulkan-loader/LICENSE"
            elif change == "directory":
                binding["declaredDirectory"] = "//foreign"
            elif change == "duplicate":
                dawn["configuredSourceLicenseBindings"].append(copy.deepcopy(binding))
            else:
                dawn["configurationEvidence"]["declaredDirectories"]["dawn_vulkan_loader_dir"] = "//foreign"
            with self.assertRaises(ValueError):
                SUBJECT["metadata_dispositions"](current, api, dawn)

    def test_configuration_bindings_do_not_prove_binary_exclusion(self):
        rows = SUBJECT["metadata_dispositions"](*self.dispositions())
        self.assertTrue(all(row["originalErrorRemoved"] is False and row["binaryExclusionProven"] is False for row in rows))

    def test_unsafe_or_existing_output_refuses_before_source_loader(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-notice-output-") as name:
            source = Path(name)
            existing = source / "notice-assembly-01"
            existing.mkdir()
            loader = Mock(side_effect=AssertionError("must not load source"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None), patch.dict(SUBJECT["DAWN"], load_dawn=loader):
                with self.assertRaises(FileExistsError):
                    SUBJECT["assemble"](source, existing)
                for bad in (source / "../notice-assembly-02", source / "notice-assembly-bad"):
                    with self.assertRaises(ValueError):
                        SUBJECT["assemble"](source, bad)
            loader.assert_not_called()


if __name__ == "__main__":
    unittest.main()
