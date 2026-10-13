"""Exercise immutable LLVM text collection and retained failure evidence."""
import base64
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-llvm-notice-texts.py")))


class LlvmNoticeTests(unittest.TestCase):
    def setUp(self):
        self.texts = {"NCSA": b"Synthetic complete NCSA text\n",
                      "LLVM-exception": b"Synthetic complete LLVM exception\n"}
        self.pins = {key: {"oid": hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest(),
                           "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                     for key, data in self.texts.items()}
        self.parent = {"verified": True, "sourceReceiptSha256": "a" * 64}
        self.globals = SUBJECT["collect"].__globals__
        for mapping, values in ((self.globals, {"LICENSES": self.pins}),
                                (SUBJECT["INPUTS"], {"checked_source": lambda path: None}),
                                (SUBJECT["SPDX"], {"verify": lambda source, name: self.parent})):
            patcher = patch.dict(mapping, values)
            patcher.start()
            self.addCleanup(patcher.stop)

    def response(self, request):
        key = request["key"]
        return json.dumps({"path": request["path"], "sha": self.pins[key]["oid"],
            "size": len(self.texts[key]), "type": "file", "encoding": "base64",
            "git_url": SUBJECT["SPDX"]["API"] + "/git/blobs/" + self.pins[key]["oid"],
            "content": base64.b64encode(self.texts[key]).decode()}).encode()

    def collect(self, source):
        output = source / "llvm-license-source-01"
        with patch.dict(self.globals, fetch=self.response):
            report = SUBJECT["collect"](source, output)
        seal = hashlib.sha256((output / "llvm-license-inventory.json").read_bytes()).hexdigest()
        return output, report, seal

    def test_only_fixed_revision_urls_are_requested(self):
        rows = SUBJECT["requests"]()
        self.assertEqual({row["key"] for row in rows}, set(self.texts))
        for row in rows:
            self.assertTrue(row["url"].endswith("?ref=" + SUBJECT["SPDX"]["REVISION"]))
            with self.assertRaisesRegex(ValueError, "compiled immutable"):
                SUBJECT["fetch"]({**row, "url": row["url"].replace("?ref=", "?branch=")})

    def test_original_objects_bytes_and_full_text_are_required(self):
        for request in SUBJECT["requests"]():
            raw = self.response(request)
            data, _ = SUBJECT["SPDX"]["decoded_license"](request["key"], raw, self.pins)
            self.assertEqual(data, self.texts[request["key"]])
            for field, value in (("sha", "0" * 40), ("size", 1), ("type", "symlink"),
                                 ("content", base64.b64encode(data[:-1]).decode())):
                changed = {**json.loads(raw), field: value}
                with self.subTest(field=field), self.assertRaises(ValueError):
                    SUBJECT["SPDX"]["decoded_license"](request["key"], json.dumps(changed).encode(), self.pins)

    def test_offline_verification_preserves_normative_scope(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-llvm-") as name:
            source = Path(name)
            _, report, seal = self.collect(source)
            texts, receipt = SUBJECT["verify"](source, "llvm-license-source-01", seal)
            self.assertEqual(texts, self.texts)
            self.assertEqual(receipt["standardTextBytes"], sum(map(len, self.texts.values())))
            self.assertEqual(report["originalSpdxIdentityEvidence"], self.parent)
            self.assertFalse(report["completeChromiumNotice"])
            self.assertFalse(report["promoted"])

    def test_modified_raw_full_text_or_receipt_is_rejected(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-llvm-") as name:
            source = Path(name)
            output, _, seal = self.collect(source)
            for filename in ("NCSA.json-data", "LLVM-exception.license-data", "llvm-license-inventory.json"):
                path = output / filename
                original = path.read_bytes()
                path.write_bytes(original + b" ")
                with self.subTest(filename=filename), self.assertRaises(ValueError):
                    SUBJECT["verify"](source, output.name, seal)
                path.write_bytes(original)

    def test_receipt_cannot_change_identity_or_claim_promotion(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-llvm-") as name:
            source = Path(name)
            output, report, _ = self.collect(source)
            for field, value in (("spdxRevision", "0" * 40), ("spdxTreeSha1", "0" * 40),
                                 ("originalSpdxIdentityEvidence", {}), ("collectionComplete", False),
                                 ("completeChromiumNotice", True), ("promoted", True)):
                raw = json.dumps({**report, field: value}).encode()
                (output / "llvm-license-inventory.json").write_bytes(raw)
                with self.subTest(field=field), self.assertRaisesRegex(ValueError, "non-promoted"):
                    SUBJECT["verify"](source, output.name, hashlib.sha256(raw).hexdigest())

    def test_failed_decode_keeps_original_response_and_incomplete_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-llvm-") as name:
            source = Path(name)
            output = source / "llvm-license-source-01"
            with patch.dict(self.globals, fetch=lambda request: b"{}"), self.assertRaises(ValueError):
                SUBJECT["collect"](source, output)
            report = json.loads((output / "llvm-license-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertEqual(report["failureType"], "ValueError")
            self.assertFalse(report["records"][0]["verified"])
            self.assertEqual((output / "NCSA.json-data").read_bytes(), b"{}")

    def test_existing_or_escaping_output_is_never_overwritten(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-llvm-") as name:
            source = Path(name)
            output = source / "llvm-license-source-01"
            output.mkdir()
            marker = output / "original"
            marker.write_bytes(b"keep")
            for invalid in (source / "../llvm-license-source-01", source / "foreign"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](source, invalid)
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](source, output)
            self.assertEqual(marker.read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
