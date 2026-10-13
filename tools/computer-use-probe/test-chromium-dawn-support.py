"""Support notices preserve actual revisions, copyright and partial coverage."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-dawn-support.py")))


class DawnSupportTests(unittest.TestCase):
    def metadata(self):
        return b"Revision: b2b9072ecbe874f5937054653ef8f2731eb0f010\nLicense: BSD-3-Clause\nLicense File: src/LICENSE\nShipped: no\n"

    def blobs(self):
        return {path: prefix + b" The Chromium Authors\n" + leader
            + b" Use of this source code is governed by a BSD-style license that can be\n"
            + leader + b" found in the LICENSE file.\n" for path, prefix, leader in zip(SUBJECT["PA_SOURCES"],
                (b"# Copyright 2023", b"// Copyright 2020"), (b"#", b"//"))}

    def test_metadata_revision_mismatch_and_no_flag_remain_original_evidence(self):
        report = SUBJECT["metadata_evidence"](self.metadata(), "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7")
        self.assertEqual(report["originalFields"]["Shipped"]["value"], "no")
        self.assertEqual(report["originalFields"]["Revision"]["value"], "b2b9072ecbe874f5937054653ef8f2731eb0f010")
        self.assertTrue(report["metadataRevisionDiffersFromActualDeps"])
        self.assertFalse(report["metadataRewritten"])
        self.assertFalse(report["shippingExclusionProven"])

    def test_altered_duplicated_or_missing_metadata_and_stale_pin_refuse(self):
        original = self.metadata()
        for data, pin in ((original.replace(b"Shipped: no", b"Shipped: yes"), "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7"),
                (original + b"License: BSD-3-Clause\n", "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7"),
                (original.replace(b"License File: src/LICENSE\n", b""), "4fe3307fb2d9f86d19777c7eb0e4809e9694dde7"),
                (original, "b2b9072ecbe874f5937054653ef8f2731eb0f010")):
            with self.subTest(data=data), self.assertRaises(ValueError):
                SUBJECT["metadata_evidence"](data, pin)

    def test_both_partition_original_headers_and_source_hashes_survive(self):
        blobs = self.blobs()
        rows = SUBJECT["partition_attribution"](blobs)
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertEqual(row["sourceSha256"], hashlib.sha256(blobs[row["sourcePath"]]).hexdigest())
            self.assertEqual(row["resolvedLicenseSource"], "LICENSE")
            self.assertIn("found in the LICENSE file.", row["originalAttribution"])

    def test_changed_attribution_year_holder_or_license_reference_refuses(self):
        original = self.blobs()
        path = SUBJECT["PA_SOURCES"][0]
        for old, new in ((b"2023", b"2024"), (b"The Chromium Authors", b"someone else"),
                (b"BSD-style license", b"Apache license"), (b"LICENSE file.", b"other file.")):
            with self.subTest(old=old), self.assertRaises(ValueError):
                SUBJECT["partition_attribution"]({**original, path: original[path].replace(old, new)})

    def test_full_notices_headers_and_partial_markers_are_preserved(self):
        rows = SUBJECT["partition_attribution"](self.blobs())
        gt = b"Google attribution\nfull permission\nfull disclaimer\n"
        chromium = b"Chromium attribution\nfull permission\nfull disclaimer\n"
        result = SUBJECT["render_fragment"](gt, chromium, rows)
        self.assertIn(gt, result)
        self.assertIn(chromium, result)
        self.assertIn(b"NOT FOR DISTRIBUTION", result)
        for row in rows:
            self.assertIn(row["originalAttribution"].encode(), result)

    def test_missing_notice_or_attribution_refuses(self):
        rows = SUBJECT["partition_attribution"](self.blobs())
        for gt, chromium, attribution in ((b"", b"license", rows), (b"license", b"", rows),
                ("not bytes", b"license", rows), (b"license", b"license", rows[:1])):
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](gt, chromium, attribution)

    def test_fragment_budget_counts_original_headers_and_separators(self):
        rows = SUBJECT["partition_attribution"](self.blobs())
        actual = SUBJECT["render_fragment"](b"license", b"other license", rows)
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(actual)):
            self.assertEqual(SUBJECT["render_fragment"](b"license", b"other license", rows), actual)
        with patch.dict(SUBJECT["render_fragment"].__globals__, MAX_FRAGMENT=len(actual) - 1), self.assertRaises(ValueError):
            SUBJECT["render_fragment"](b"license", b"other license", rows)

    def test_unsafe_or_existing_output_refuses_before_source_loading(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-support-output-") as name:
            source = Path(name)
            existing = source / "dawn-support-coverage-01"
            existing.mkdir()
            loader = Mock(side_effect=AssertionError("must not load source"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None), patch.dict(SUBJECT["DAWN"], load_dawn=loader):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, existing)
                for bad in (source / "../dawn-support-coverage-02", source / "dawn-support-coverage-bad"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, bad)
            loader.assert_not_called()


if __name__ == "__main__":
    unittest.main()
