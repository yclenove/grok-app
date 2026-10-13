"""Bounded fixed-source collection, inert data, retained late failure."""
import base64
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-generated-input-sources.py")))


class GeneratedInputSourceTests(unittest.TestCase):
    def test_eight_immutable_urls_and_independent_git_pins(self):
        rows = SUBJECT["requests"]()
        self.assertEqual(len(rows), 8)
        self.assertEqual(len({row["path"] for row in rows}), 8)
        for row in rows:
            self.assertIn("/+/" + SUBJECT["COMMIT"] + "/", row["url"])
            self.assertEqual(row["gitObjectSha1"], SUBJECT["PINS"][row["path"]])
            self.assertTrue(row["url"].endswith("?format=TEXT"))

    def test_arbitrary_url_revision_or_git_blob_rejected_before_network(self):
        row = SUBJECT["requests"]()[0]
        for key, value in (("url", "https://example.test/file"), ("gitObjectSha1", "a" * 40), ("path", "../other")):
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "eight compiled"):
                SUBJECT["fetch"]({**row, key: value})

    def test_decoded_bytes_match_real_git_object_without_execution(self):
        contents = b"raise RuntimeError('do not execute')\n"
        oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
        decoded, identity = SUBJECT["decode"](base64.b64encode(contents), oid)
        self.assertEqual(decoded, contents)
        self.assertEqual(identity["sha256"], hashlib.sha256(contents).hexdigest())
        with self.assertRaises(ValueError):
            SUBJECT["decode"](base64.b64encode(contents + b"changed"), oid)

    def test_invalid_base64_empty_oversize_or_non_utf8_reject(self):
        for raw in (b"", b"invalid!", b" " * (SUBJECT["MAX_RAW"] + 1), base64.b64encode(b"")):
            with self.subTest(length=len(raw)), self.assertRaises(ValueError):
                SUBJECT["decode"](raw, "a" * 40)
        contents = b"\xff"
        oid = hashlib.sha1(b"blob 1\0" + contents).hexdigest()
        with self.assertRaises(UnicodeDecodeError):
            SUBJECT["decode"](base64.b64encode(contents), oid)

    def test_source_allowlist_must_match_independent_tree_before_fetch(self):
        with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: {"index": {}, "mainSha256": "fixture"}), self.assertRaisesRegex(ValueError, "independent sealed"):
            SUBJECT["checked_inputs"](Path("fixture"))

    def test_foreign_existing_or_escaping_output_rejected(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-input-source-") as name:
            root = Path(name)
            for output in (root / "other", root / "../generated-input-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, output)
            output = root / "generated-input-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_late_decode_failure_keeps_raw_and_partial_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-input-source-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: "fixture", fetch=lambda *_: b"invalid!"), self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "generated-input-source-01")
            report = json.loads((root / "generated-input-source-01/generated-input-inventory.json").read_text())
            self.assertFalse(report["collectionComplete"])
            self.assertFalse(report["records"][0]["verified"])
            self.assertEqual((root / "generated-input-source-01" / report["records"][0]["rawFile"]).read_bytes(), b"invalid!")
            for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "generatedInputsReproduced", "originalLicenseErrorsResolved", "promoted"):
                self.assertFalse(report[key])


if __name__ == "__main__":
    unittest.main()
