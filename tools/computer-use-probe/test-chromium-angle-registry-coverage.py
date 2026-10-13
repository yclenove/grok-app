"""Registry input identity and complete exception text cannot be weakened."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-angle-registry-coverage.py")))
ANGLE = SUBJECT["GENERATION"]["ANGLE"]
XML_PATHS = [ANGLE + path for path in SUBJECT["GENERATION"]["XML_PATHS"]]


class AngleRegistryCoverageTests(unittest.TestCase):
    def xml(self, exception=False):
        declaration = "Copyright 2013-2026 The Khronos Group Inc.\nSPDX-License-Identifier: Apache-2.0"
        if exception:
            declaration = '''Copyright 2013-2025 The Khronos Group Inc.
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
http://www.apache.org/licenses/LICENSE-2.0
distributed under the License is distributed on an "AS IS" BASIS
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND
---- Exceptions to the Apache 2.0 License: ----
As an exception, if you use this Software to generate code and portions of
this Software are embedded into the generated code as a result, you may
redistribute such product without providing attribution as would otherwise
be required by Sections 4(a), 4(b) and 4(d) of the License.
software that is licensed under the GPLv2 or the LGPL v2.0 or 2.1
patent provision (Section 3), the indemnity provision (Section 9)
but only in their entirety and only with respect to the Combined Software.'''
        return ('<?xml version="1.0" encoding="UTF-8"?>\n<registry>\n    <comment>\n' + declaration
            + '\n    </comment>\n<types/>\n</registry>\n').encode()

    def test_xml_attribution_and_complete_generation_exception_remain_original_bytes(self):
        for path in XML_PATHS:
            exception = "OpenCL-Docs" in path
            data = self.xml(exception)
            report = SUBJECT["xml_notice"](data, path)
            self.assertIn(report["originalAttributionText"].encode(), data)
            self.assertEqual(report["originalExceptionsPreserved"], exception)
            self.assertFalse(report["exceptionUsedToExcludeAttribution"])
            self.assertEqual(report["sourceSha256"], hashlib.sha256(data).hexdigest())
        egl = self.xml().replace(b"<comment>", b"<!--").replace(b"</comment>", b"-->")
        self.assertEqual(SUBJECT["xml_notice"](egl, XML_PATHS[0])["noticeKind"], "apache_spdx")

    def test_truncated_or_changed_exception_waiver_spdx_and_attribution_refuse(self):
        for old, new in ((b"Sections 4(a), 4(b) and 4(d)", b"Section 4"),
                (b"but only in their entirety", b"except selectively"), (b"</comment>", b""),
                (b"The Khronos Group Inc.", b"Another author")):
            with self.subTest(old=old), self.assertRaises(ValueError):
                SUBJECT["xml_notice"](self.xml(True).replace(old, new), XML_PATHS[1])
        with self.assertRaises(ValueError):
            SUBJECT["xml_notice"](self.xml().replace(b"Apache-2.0", b"MIT"), XML_PATHS[0])

    def test_xml_is_text_only_and_does_not_follow_external_entities_or_alternative_roots(self):
        for data in (b"", self.xml().replace(b"<registry>", b"<!DOCTYPE registry SYSTEM 'https://example.test'>\n<registry>"),
                self.xml().replace(b"UTF-8", b"UTF-16"), b"x" * (SUBJECT["INPUTS"]["TREE"]["MAX_BLOB"] + 1)):
            with self.assertRaises(ValueError):
                SUBJECT["xml_notice"](data, XML_PATHS[0])

    def test_source_version_difference_is_retained_instead_of_rewriting_recorded_hash(self):
        path, data = XML_PATHS[0], b"original fixed revision\n"
        relative = path[len(ANGLE):]
        recorded = {relative: "0" * 32}
        report = SUBJECT["xml_generation_match"](path, data, recorded)
        self.assertEqual(report["status"], "fixed_revision_differs_from_generation_record")
        self.assertFalse(report["matchesRecordedGenerationInput"])
        self.assertEqual(recorded, {relative: "0" * 32})
        self.assertFalse(report["md5IsSourceIdentity"])
        self.assertFalse(report["generatorReproduced"])

    def test_raw_or_windows_normalized_generation_match_does_not_claim_binary_use(self):
        path, data = XML_PATHS[0], b"fixed input\r\n"
        relative = path[len(ANGLE):]
        for value in (hashlib.md5(data).hexdigest(), hashlib.md5(data.replace(b"\r\n", b"\n")).hexdigest()):
            report = SUBJECT["xml_generation_match"](path, data, {relative: value})
            self.assertTrue(report["matchesRecordedGenerationInput"])
            self.assertFalse(report["binaryUseProven"])
        with self.assertRaises(ValueError):
            SUBJECT["xml_generation_match"](ANGLE + "other.xml", data, {relative: value})

    def test_header_identical_blob_is_distinct_from_same_basename_or_symlink(self):
        original, other = b"original header\n", b"different version\n"
        oid = hashlib.sha1(b"blob " + str(len(original)).encode() + b"\0" + original).hexdigest()
        other_oid = hashlib.sha1(b"blob " + str(len(other)).encode() + b"\0" + other).hexdigest()
        trees = {"egl-registry": [
            {"path": "api/EGL/egl.h", "mode": "100644", "kind": "blob", "gitObjectSha1": oid},
            {"path": "api/EGL/eglext.h", "mode": "100644", "kind": "blob", "gitObjectSha1": other_oid},
            {"path": "api/EGL/link.h", "mode": "120000", "kind": "blob", "gitObjectSha1": oid}]}
        headers = {ANGLE + "include/EGL/egl.h": original, ANGLE + "include/EGL/eglext.h": b"copied old version\n"}
        report = SUBJECT["copied_header_matches"](headers, trees)
        self.assertEqual(report[0]["status"], "identical_fixed_registry_tree_blob")
        self.assertEqual(len(report[0]["identicalFixedRegistryTreeBlobs"]), 1)
        self.assertEqual(report[1]["status"], "no_identical_blob_in_these_three_fixed_trees")
        self.assertEqual(len(report[1]["sameBasenameApiCandidates"]), 1)
        self.assertTrue(all(not row["historicalCopyChainProven"] and not row["completeHeaderProvenance"] for row in report))

    def test_all_five_original_notices_and_full_license_survive_fragment_and_budget(self):
        notices = [SUBJECT["xml_notice"](self.xml("OpenCL-Docs" in path), path) for path in XML_PATHS]
        apache = b"full original Apache license\npermission\ndisclaimer\n"
        fragment = SUBJECT["render_fragment"](notices, apache)
        self.assertIn(apache, fragment)
        self.assertIn(b"NOT FOR DISTRIBUTION", fragment)
        for row in notices:
            self.assertIn(row["originalAttributionText"].encode(), fragment)
        for changed, text in ((notices[:4], apache), (notices[:4] + notices[:1], apache), (notices, b"")):
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](changed, text)
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(fragment) - 1), self.assertRaises(ValueError):
            SUBJECT["render_fragment"](notices, apache)

    def test_header_profile_uses_its_original_full_correspondence_not_registry_subset(self):
        inputs = {"mainSha256": "main", "index": {"header": "oid"}}
        correspondence = [{"target": str(i)} for i in range(12)]
        checked = Mock(return_value=({"header": b"original"}, "header-seal"))
        with patch.dict(SUBJECT["GENERATION"]["SOURCES"], checked_inputs=lambda *_: (inputs, "checkout", correspondence)), \
                patch.dict(SUBJECT["GENERATION"]["INTEGRATION"], checked_profile=checked), \
                patch.dict(SUBJECT["SUPPLEMENT"], header_paths=lambda index: index):
            self.assertEqual(SUBJECT["checked_headers"](Path("fixture"), inputs, "checkout"), ({"header": b"original"}, "header-seal"))
            self.assertIs(checked.call_args.args[4], correspondence)
            with self.assertRaisesRegex(ValueError, "same original"):
                SUBJECT["checked_headers"](Path("fixture"), inputs, "other-checkout")
        self.assertEqual(checked.call_count, 1)

    def test_output_scope_and_overwrite_reject_before_loading_sources(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-registry-coverage-") as name:
            root = Path(name)
            loader = Mock(side_effect=AssertionError("must not load source"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), patch.dict(SUBJECT["COLLECTOR"], checked_inputs=loader):
                for output in (root / "foreign", root / "../angle-registry-coverage-01"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](root, output)
                output = root / "angle-registry-coverage-01"
                output.mkdir()
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](root, output)
            loader.assert_not_called()


if __name__ == "__main__":
    unittest.main()
