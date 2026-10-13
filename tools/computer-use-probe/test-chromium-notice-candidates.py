"""Generator selection must retain unknown fields and every original text gap."""
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-notice-candidates.py")))


def row(shipped="yes", errors=None, metadata="third_party/example/README.chromium", paths=None):
    return {"name": "Example", "directory": "third_party/example", "license": "MIT", "shippedMetadata": shipped,
            "metadata": metadata, "errors": errors or [], "availableLicensePaths": ["third_party/example/LICENSE"] if paths is None else paths}


def source(data):
    return {"payload": data, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


class NoticeCandidateTests(unittest.TestCase):
    def candidates(self, rows):
        return SUBJECT["select_candidates"]({"entries": rows, "directoryErrors": [{"directory": "third_party/missing"}]},
                                             {"YES": "yes", "NO": "no"})

    def inputs(self):
        return {"files": {"LICENSE": source(b"Chromium root license\n"),
                          "third_party/example/LICENSE": source(b"Copyright ORIGINAL OWNER\nPermission and full disclaimer.\n")},
                "index": {"LICENSE": {"mode": "100644"}, "third_party/example/LICENSE": {"mode": "100644"}}}

    def test_only_exact_no_is_excluded_and_unknown_case_never_normalized(self):
        rows = [row(value) for value in ("yes", "no", "Yes", "No", "maybe")]
        original = copy.deepcopy(rows)
        result = self.candidates(rows)
        self.assertEqual([item["shippedMetadata"] for item in result["generatorCandidates"]], ["yes", "Yes", "No", "maybe"])
        self.assertEqual([item["shippedMetadata"] for item in result["exactNoExcluded"]], ["no"])
        self.assertEqual([item["shippedMetadata"] for item in result["nonstandardShippingValues"]], ["Yes", "No", "maybe"])
        self.assertEqual(rows, original)

    def test_bad_readmes_omitted_but_special_case_errors_and_empty_text_retained(self):
        broken = row(errors=["missing text"], paths=[])
        special = row(errors=["missing text"], paths=[], metadata="licenses.py SPECIAL_CASES")
        unknown = row("Yes", paths=[])
        result = self.candidates([broken, special, unknown])
        self.assertEqual(result["invalidReadmeRecordsOmitted"], [broken])
        self.assertEqual(result["generatorCandidates"], [special, unknown])
        self.assertEqual(result["specialCaseRecordsWithErrorsRetained"], [special])
        self.assertEqual(result["candidateRecordsWithoutFullText"], [special, unknown])
        self.assertEqual(result["directoryErrorsRetained"], [{"directory": "third_party/missing"}])

    def test_exact_pinned_source_rules_required_not_inferred_from_comments(self):
        contents = ("def _DiscoverMetadatas(args):\n    for m in []:\n        if args.shipped_only and m['Shipped'] == NO:\n            continue\n"
                    "def ParseDir():\n    if special:\n        return [directory_metadata], errors\n"
                    "    if meta_errors:\n        errors.append('error')\n        continue\n").encode()
        with patch.dict(SUBJECT["POLICY"], POLICY_SHA=hashlib.sha256(contents).hexdigest()):
            result = SUBJECT["selection_rule"](contents)
            self.assertTrue(result["comparisonIsCaseSensitive"])
            self.assertFalse(result["sourceFunctionsExecuted"])
            self.assertTrue(result["specialCaseErrorsDoNotOmitRecord"])
        for changed in (contents.replace(b"== NO", b"!= YES"), contents.replace(b"return [directory_metadata], errors", b"return [], errors"),
                        contents.replace(b"if meta_errors:", b"if other_errors:")):
            with patch.dict(SUBJECT["POLICY"], POLICY_SHA=hashlib.sha256(changed).hexdigest()), self.assertRaises(ValueError):
                SUBJECT["selection_rule"](changed)
        with self.assertRaisesRegex(ValueError, "reviewed Chromium"):
            SUBJECT["selection_rule"](contents)

    def test_preview_retains_verbatim_text_and_marks_partial_not_distributable(self):
        inputs = self.inputs()
        candidates = self.candidates([row("Yes")])
        before = copy.deepcopy((inputs, candidates))
        preview, bindings = SUBJECT["render_preview"](candidates, inputs)
        self.assertEqual((inputs, candidates), before)
        self.assertIn(b"PARTIAL SOURCE NOTICE PREVIEW - NOT FOR DISTRIBUTION", preview)
        self.assertIn(b"Raw Shipped: Yes", preview)
        for path, item in inputs["files"].items():
            self.assertIn(item["payload"], preview)
            self.assertTrue(any(binding["sourcePath"] == path and binding["sha256"] == item["sha256"] for binding in bindings))
        self.assertEqual(len(bindings), 2)

    def test_missing_or_changed_real_source_cannot_be_filled_with_neighbor_license(self):
        candidates = self.candidates([row()])
        for change in ("missing", "changed", "symlink", "size", "empty", "foreign"):
            inputs = self.inputs()
            path = "third_party/example/LICENSE"
            if change == "missing":
                del inputs["files"][path]
            elif change == "changed":
                inputs["files"][path]["payload"] += b" "
            elif change == "symlink":
                inputs["index"][path]["mode"] = "120000"
            elif change == "size":
                inputs["files"][path]["bytes"] += 1
            elif change == "empty":
                inputs["files"][path] = source(b"")
            else:
                inputs["files"]["third_party/neighbor/LICENSE"] = inputs["files"].pop(path)
            with self.subTest(change=change), self.assertRaises(ValueError):
                SUBJECT["render_preview"](candidates, inputs)

    def test_generated_archive_source_and_chromium_root_have_same_digest_contract(self):
        inputs = self.inputs()
        inputs["index"]["third_party/example/LICENSE"]["mode"] = "archive_regular_file"
        SUBJECT["render_preview"](self.candidates([row()]), inputs)
        for change in ("missing", "tamper", "size", "symlink"):
            inputs = self.inputs()
            if change == "missing":
                del inputs["files"]["LICENSE"]
            elif change == "size":
                inputs["files"]["LICENSE"]["bytes"] += 1
            elif change == "symlink":
                inputs["index"]["LICENSE"]["mode"] = "120000"
            else:
                inputs["files"]["LICENSE"]["payload"] += b" "
            with self.assertRaises(ValueError):
                SUBJECT["render_preview"](self.candidates([row()]), inputs)

    def test_notice_budget_includes_component_headers_and_separators(self):
        candidates, inputs = self.candidates([row()]), self.inputs()
        preview, _ = SUBJECT["render_preview"](candidates, inputs)
        with patch.dict(SUBJECT["render_preview"].__globals__, MAX_NOTICE=len(preview)):
            self.assertEqual(SUBJECT["render_preview"](candidates, inputs)[0], preview)
        for budget in (len(preview) - 1, 1):
            with patch.dict(SUBJECT["render_preview"].__globals__, MAX_NOTICE=budget), self.assertRaises(ValueError):
                SUBJECT["render_preview"](candidates, inputs)

    def test_foreign_escaping_or_existing_output_does_not_overwrite_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-notice-preview-") as name:
            source_dir = Path(name)
            output = source_dir / "notice-candidates-01"
            output.mkdir()
            (output / "original.txt").write_bytes(b"keep")
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None):
                for invalid in (source_dir / "foreign", source_dir / "../notice-candidates-01"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source_dir, invalid)
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source_dir, output)
            self.assertEqual((output / "original.txt").read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
