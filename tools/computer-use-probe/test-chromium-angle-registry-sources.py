"""Exact registry declarations, source-only collection and retained failures."""
import copy
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-angle-registry-sources.py")))


class AngleRegistrySourceTests(unittest.TestCase):
    def component(self, key):
        prefix, repository, revision = SUBJECT["PINS"][key]
        return {"checkoutPath": prefix, "repository": repository, "revision": revision, "treeEntries": 10,
            "selectedFiles": [{"path": path} for path in ("LICENSE", *SUBJECT["EXTRA_PATHS"][key])],
            "sourceBytes": 100, "reviewedExtraPaths": sorted(SUBJECT["EXTRA_PATHS"][key]),
            "missingParentOrExtraPaths": [], "collectionComplete": True, "generatorExecuted": False, "promoted": False}

    def test_compiled_sources_are_three_original_registry_revisions_and_five_xml_paths(self):
        self.assertEqual(set(SUBJECT["PINS"]), {"egl-registry", "opencl-docs", "opengl-registry"})
        self.assertEqual(sum(map(len, SUBJECT["EXTRA_PATHS"].values())), 5)
        self.assertEqual(SUBJECT["PINS"]["egl-registry"][2], "7dea2ed79187cd13f76183c4b9100159b9e3e071")
        self.assertEqual(SUBJECT["PINS"]["opencl-docs"][2], "5b4ca15f0e5a5be87b56b99f652f728e05cab587")
        self.assertEqual(SUBJECT["PINS"]["opengl-registry"][2], "9cb90ca4902d588bef3c830fbb1da484893bd5fb")
        for key, (prefix, repository, _) in SUBJECT["PINS"].items():
            self.assertEqual(prefix, "third_party/angle/third_party/" + SUBJECT["NAMES"][key] + "/src")
            self.assertEqual(repository, "https://chromium.googlesource.com/external/github.com/KhronosGroup/" + SUBJECT["NAMES"][key])

    def test_complete_xml_summary_does_not_claim_manifest_presence_or_binary_exclusion(self):
        for key in SUBJECT["PINS"]:
            component = self.component(key)
            original = copy.deepcopy(component)
            summary = SUBJECT["component_summary"](key, component, "seal")
            self.assertEqual(summary["receiptSha256"], "seal")
            self.assertEqual(summary["declaredCondition"], "not build_with_chromium")
            self.assertFalse(summary["binaryExclusionProven"])
            self.assertEqual(component, original)

    def test_missing_xml_foreign_pin_extra_request_or_true_execution_claim_refuses(self):
        for changed in ({"revision": "0" * 40}, {"repository": "https://example.test/other"}, {"collectionComplete": False},
                {"selectedFiles": [{"path": "LICENSE"}]}, {"missingParentOrExtraPaths": ["api/egl.xml"]},
                {"reviewedExtraPaths": ["other.xml"]}, {"generatorExecuted": True}, {"promoted": True}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                SUBJECT["component_summary"]("egl-registry", {**self.component("egl-registry"), **changed}, "seal")

    def test_unsafe_or_existing_output_refuses_before_read_or_network(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-registry-output-") as name:
            root = Path(name)
            loader = Mock(side_effect=AssertionError("must not read sources"))
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=loader):
                for output in (root / "foreign", root / "../angle-registry-license-source-01"):
                    with self.assertRaises(ValueError):
                        SUBJECT["collect"](root, output)
                output = root / "angle-registry-license-source-01"
                output.mkdir()
                with self.assertRaises(FileExistsError):
                    SUBJECT["collect"](root, output)
            loader.assert_not_called()

    def test_preflight_identity_failure_does_not_create_output(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-registry-preflight-") as name:
            root = Path(name)
            output = root / "angle-registry-license-source-01"
            loader = Mock(side_effect=ValueError("wrong source identity"))
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=loader), self.assertRaisesRegex(ValueError, "identity"):
                SUBJECT["collect"](root, output)
            self.assertFalse(output.exists())

    def test_late_failure_keeps_partial_data_and_completed_earlier_components(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-registry-partial-") as name:
            root = Path(name)
            def collect(key, output, _references, pins, extra_paths):
                self.assertEqual(pins, SUBJECT["PINS"])
                self.assertEqual(extra_paths, SUBJECT["EXTRA_PATHS"][key])
                if key == "opencl-docs":
                    (output / "failed-source.data").write_bytes(b"retain original failure")
                    raise ValueError("bounded fixture failure")
                return self.component(key)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main", "references": []}, "checkout", [], [])), \
                    patch.dict(SUBJECT["EXTERNAL"], collect_component=collect), patch.dict(SUBJECT["INPUTS"], receipt=lambda *_: ({}, "seal")), \
                    self.assertRaisesRegex(ValueError, "fixture failure"):
                SUBJECT["collect"](root, root / "angle-registry-license-source-01")
            output = root / "angle-registry-license-source-01"
            report = json.loads((output / "angle-registry-source-inventory.json").read_text())
            self.assertEqual([row["component"] for row in report["components"]], ["egl-registry"])
            self.assertFalse(report["collectionComplete"])
            self.assertEqual((output / "failed-source.data").read_bytes(), b"retain original failure")
            for key in ("generatorExecuted", "metadataPathsAliased", "shippingDecisionsMade", "releaseReady", "promoted"):
                self.assertFalse(report[key])

    def test_success_keeps_all_three_source_roots_and_original_absence_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-registry-success-") as name:
            root = Path(name)
            declarations = [{"originalManifestCorrespondence": [{"status": "exact_manifest_entry_absent"}]}]
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main", "references": []}, "checkout", [], declarations)), \
                    patch.dict(SUBJECT["EXTERNAL"], collect_component=lambda key, *_a, **_k: self.component(key)), \
                    patch.dict(SUBJECT["INPUTS"], receipt=lambda *_: ({}, "seal")):
                report = SUBJECT["collect"](root, root / "angle-registry-license-source-01")
            self.assertTrue(report["collectionComplete"])
            self.assertEqual(len(report["components"]), 3)
            self.assertEqual(report["originalRegistryDeclarations"], declarations)
            self.assertFalse(report["completeChromiumNotice"])
            self.assertFalse(report["binarySbom"])
            self.assertFalse(report["promoted"])


if __name__ == "__main__":
    unittest.main()
