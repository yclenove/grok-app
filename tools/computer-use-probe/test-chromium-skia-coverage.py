"""Fail closed on source/checkout/notice mutations, without network or GN."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-skia-coverage.py")))


class SkiaTests(unittest.TestCase):
    def inputs(self, path="third_party/skia/LICENSE", data=b"complete original license"):
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        return {"index": {path: {"mode": "100644", "gitObjectSha1": oid}},
                "files": {path: {"gitObjectSha1": oid, "payload": data, "bytes": len(data),
                    "sha256": hashlib.sha256(data).hexdigest()}}}

    def test_gn_inventory_keeps_dotfile_and_other_gn_but_never_gitlink_or_other_root(self):
        index = {path: {"mode": mode, "gitObjectSha1": str(n)} for n, (path, mode) in enumerate([
            ("third_party/skia/.gn", "100644"), ("third_party/skia/build/foo.gni", "100755"),
            ("third_party/skia/links/BUILD.gn", "120000"), ("third_party/skia/submodule.gn", "160000"),
            ("skia/BUILD.gn", "100644"), ("third_party/skia/file.cc", "100644")])}
        self.assertEqual(SUBJECT["gn_inventory"](index, SUBJECT["ROOT"]),
            {"third_party/skia/.gn": "0", "third_party/skia/build/foo.gni": "1"})

    def test_all_four_real_checkout_identities_required_and_duplicates_reject(self):
        pins = SUBJECT["INPUTS"]["EXTERNAL"]["GPU_PINS"]
        path, repository, revision = pins["dawn"]
        targets = [{"target": name, "manifest": {"src/" + path: {"repository": repository, "revision": revision}}}
            for name in ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")]
        report = {"targets": targets, "checkoutReceiptSha256": "sealed checkout"}
        with patch.dict(SUBJECT["SOURCES"]["CHECKOUTS"], inspect=lambda _source: report):
            rows, seal = SUBJECT["checkout_matches"](Path("unused"), pins)
            self.assertEqual(len(rows), 4)
            self.assertEqual(seal, "sealed checkout")
            targets[2]["manifest"]["src/" + path]["revision"] = "0" * 40
            with self.assertRaises(ValueError):
                SUBJECT["checkout_matches"](Path("unused"), pins)
            report["targets"] = targets + [targets[0]]
            with self.assertRaises(ValueError):
                SUBJECT["checkout_matches"](Path("unused"), pins)

    def test_retained_directory_error_is_not_erased_by_real_chromium_binding(self):
        directory = "third_party/skia/third_party/wuffs"
        error = {"directory": directory, "error": "missing metadata"}
        path = directory + "/BUILD.gn"
        rows = SUBJECT["retained_skia_errors"]([error], {path: {"mode": "100644"}},
            {path: b'  sources = [ "../externals/wuffs/source.c" ]\n'}, [{"component": "wuffs"}])
        self.assertEqual(rows[0]["originalDirectoryError"], error)
        self.assertEqual(rows[0]["chromiumConsumerBindings"], [{"component": "wuffs"}])
        self.assertFalse(rows[0]["originalErrorRemoved"])
        self.assertFalse(rows[0]["wholeDirectoryNoticeCovered"])
        self.assertFalse(rows[0]["referenceAbsenceMeansNotShipped"])
        self.assertEqual(rows[0]["standaloneSourceReferences"][0]["line"], 1)

    def test_build_glue_copyright_preserved_and_does_not_license_external_library(self):
        notice = b"# Copyright 2018 Google LLC\n# Use is governed by a BSD-style license in the LICENSE file.\n"
        path = "third_party/skia/third_party/wuffs/BUILD.gn"
        rows = SUBJECT["wrapper_notices"]({path: notice + b'library = "../externals/wuffs"\n'}, {"sourcePath": "license"})
        self.assertEqual(rows[0]["originalAttributionText"].encode(), notice)
        self.assertFalse(rows[0]["externalLibraryNoticeCovered"])

    def test_fragment_preserves_full_original_license_and_rejects_changed_payload(self):
        inputs = self.inputs(data=b"copyright\nfull permission\nfull disclaimer\n")
        binding = SUBJECT["SUPPLEMENT"]["source_license"](inputs, "third_party/skia/LICENSE")
        groups = [{"licenses": [binding]}]
        result = SUBJECT["render_fragment"](groups, [], inputs)
        self.assertIn(b"copyright\nfull permission\nfull disclaimer\n", result)
        inputs["files"][binding["sourcePath"]]["payload"] = b"changed license"
        with self.assertRaises(ValueError):
            SUBJECT["render_fragment"](groups, [], inputs)

    def test_missing_real_consumer_cannot_be_replaced_by_standalone_external_reference(self):
        with self.assertRaises(ValueError):
            SUBJECT["dependency_bindings"](self.inputs(), {"skia/BUILD.gn": b'"../externals/dawn"\n'})

    def test_changed_dawn_default_refuses_configuration_report(self):
        with self.assertRaises(ValueError):
            SUBJECT["configuration"]({"skia/features.gni": b"skia_use_dawn = false\n", "skia/BUILD.gn": b""})

    def test_unsafe_or_existing_output_rejects_before_collecting_or_recomputing(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-skia-contract-") as name:
            source = Path(name)
            root = source / "skia-coverage-01"
            root.mkdir()
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, root)
                for output in (source / "../skia-coverage-02", source / "skia-coverage-bad"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, output)


if __name__ == "__main__":
    unittest.main()
