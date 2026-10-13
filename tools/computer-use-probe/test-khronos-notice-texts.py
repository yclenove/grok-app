"""Notice evidence must preserve real attributions and reject altered sources."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-khronos-notice-texts.py")))


def encoded(value):
    return json.dumps(value).encode()


class KhronosNoticeTests(unittest.TestCase):
    def setUp(self):
        # Small synthetic licenses exercise the complete byte/identity contract;
        # actual immutable SPDX text is checked by the separate offline CLI.
        self.texts = {"Apache-2.0": b"Apache synthetic complete license\n",
                      "MIT": b"Copyright (c) <year> <copyright holders>\nMIT synthetic license\n"}
        self.pins = {name: {"oid": hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest(),
                            "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                     for name, data in self.texts.items()}
        self.globals = SUBJECT["collect"].__globals__
        self.patcher = patch.dict(self.globals, LICENSES=self.pins)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)
        self.headers = {"headers": []}
        for index, license_id in enumerate(["Apache-2.0"] * 5 + ["MIT"] * 2 + [None]):
            attribution = "Copyright 2013-2026 The Khronos Group Inc.\n"
            attribution += "SPDX-License-Identifier: " + license_id if license_id else 'Permission and disclaimer: "AS IS"'
            self.headers["headers"].append({"sourcePath": "angle/header-" + str(index) + ".h",
                "originalAttributionText": attribution, "sourceSha256": str(index) * 64,
                "attributionSha256": hashlib.sha256(attribution.encode()).hexdigest(),
                "copyrightYears": "2013-2026", "declaredSpdx": license_id,
                "fullLicenseTextInBlock": license_id is None})

    def metadata(self, key):
        if key == "tag-ref":
            return {"ref": "refs/tags/" + SUBJECT["TAG"], "object": {
                "sha": SUBJECT["TAG_OBJECT"], "type": "tag", "url": SUBJECT["API"] + "/git/tags/" + SUBJECT["TAG_OBJECT"]}}
        if key == "tag-object":
            return {"sha": SUBJECT["TAG_OBJECT"], "tag": SUBJECT["TAG"], "object": {
                "sha": SUBJECT["REVISION"], "type": "commit", "url": SUBJECT["API"] + "/git/commits/" + SUBJECT["REVISION"]}}
        return {"sha": SUBJECT["REVISION"], "tree": {
            "sha": SUBJECT["TREE_OBJECT"], "url": SUBJECT["API"] + "/git/trees/" + SUBJECT["TREE_OBJECT"]}}

    def license_row(self, name):
        pin = self.pins[name]
        return {"path": "text/" + name + ".txt", "sha": pin["oid"], "size": pin["bytes"], "type": "file",
                "git_url": SUBJECT["API"] + "/git/blobs/" + pin["oid"], "encoding": "base64",
                "content": base64.b64encode(self.texts[name]).decode() + "\n"}

    def response(self, request):
        key = request["key"]
        return encoded(self.license_row(key) if key in self.pins else self.metadata(key))

    def collected(self, source):
        output = source / "khronos-license-source-01"
        with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None), patch.dict(self.globals,
                checked_headers=lambda path: (self.headers, SUBJECT["CONSUMER_SHA"]), fetch=self.response):
            report = SUBJECT["collect"](source, output)
        seal = hashlib.sha256((output / "khronos-license-inventory.json").read_bytes()).hexdigest()
        return output, report, seal

    def verified(self, source, seal):
        with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None), patch.dict(self.globals,
                checked_headers=lambda path: (self.headers, SUBJECT["CONSUMER_SHA"]), SOURCE_SEAL=seal):
            return SUBJECT["verify"](source, "khronos-license-source-01")

    def test_metadata_rejects_tag_commit_tree_or_type_substitution(self):
        for key, field in (("tag-ref", "ref"), ("tag-object", "sha"), ("commit", "sha")):
            row = self.metadata(key)
            SUBJECT["checked_metadata"](key, encoded(row))
            row[field] = "foreign"
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["checked_metadata"](key, encoded(row))
        row = self.metadata("tag-object")
        row["object"]["type"] = "tree"
        with self.assertRaises(ValueError):
            SUBJECT["checked_metadata"]("tag-object", encoded(row))
        row = self.metadata("commit")
        row["tree"]["sha"] = "0" * 40
        with self.assertRaises(ValueError):
            SUBJECT["checked_metadata"]("commit", encoded(row))

    def test_license_file_identity_bytes_and_digest_are_all_required(self):
        for name in self.pins:
            row = self.license_row(name)
            self.assertEqual(SUBJECT["decoded_license"](name, encoded(row))[0], self.texts[name])
            for field, value in (("path", "text/other.txt"), ("sha", "0" * 40), ("size", 1),
                                 ("type", "symlink"), ("encoding", "raw"), ("git_url", "https://foreign"),
                                 ("content", base64.b64encode(self.texts[name] + b" ").decode())):
                changed = {**row, field: value}
                with self.subTest(name=name, field=field), self.assertRaises(ValueError):
                    SUBJECT["decoded_license"](name, encoded(changed))

    def test_license_bounds_utf8_and_base64_are_not_normalized(self):
        for data in (b"", b"[]", b"\xff"):
            with self.assertRaises((ValueError, UnicodeError)):
                SUBJECT["decoded_license"]("MIT", data)
        row = self.license_row("MIT")
        row["content"] += "!"
        with self.assertRaises(ValueError):
            SUBJECT["decoded_license"]("MIT", encoded(row))
        with patch.dict(self.globals, MAX_RAW=4), self.assertRaises(ValueError):
            SUBJECT["decoded_license"]("MIT", encoded(self.license_row("MIT")))
        with patch.dict(self.globals, MAX_TEXT=1), self.assertRaises(ValueError):
            SUBJECT["decoded_license"]("MIT", encoded(self.license_row("MIT")))

    def test_only_compiled_public_requests_allowed_before_network(self):
        for request in ({"key": "MIT", "url": "https://foreign"}, {**SUBJECT["requests"]()[0], "url": SUBJECT["API"]}):
            with self.assertRaisesRegex(ValueError, "compiled immutable"):
                SUBJECT["fetch"](request)

    def test_fragment_keeps_each_copyright_inline_text_and_standard_placeholder(self):
        original = copy.deepcopy((self.headers, self.texts))
        fragment, bindings = SUBJECT["render_fragment"](self.headers, self.texts)
        self.assertEqual((self.headers, self.texts), original)
        self.assertEqual(len(bindings), 7)
        self.assertIn(b"Copyright (c) <year> <copyright holders>", fragment)
        for row in self.headers["headers"]:
            self.assertIn(row["originalAttributionText"].encode(), fragment)
            self.assertIn(row["sourcePath"].encode(), fragment)
        for data in self.texts.values():
            self.assertIn(data, fragment)
        self.assertIn(b"not a complete Chromium NOTICE", fragment)

    def test_fragment_rejects_missing_or_changed_license_and_header_scope(self):
        for texts in ({"MIT": self.texts["MIT"]}, {**self.texts, "MIT": self.texts["MIT"][:-1]}):
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](self.headers, texts)
        for mutation in ("missing", "foreign", "inline"):
            headers = copy.deepcopy(self.headers)
            if mutation == "missing":
                headers["headers"].pop()
            elif mutation == "foreign":
                headers["headers"][0]["declaredSpdx"] = "BSD-3-Clause"
            else:
                headers["headers"][-1]["fullLicenseTextInBlock"] = False
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                SUBJECT["render_fragment"](headers, self.texts)

    def test_collect_and_offline_verify_require_original_raw_sources_and_fragment(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-khronos-") as name:
            source = Path(name)
            output, report, seal = self.collected(source)
            result = self.verified(source, seal)
            self.assertTrue(result["verified"])
            self.assertFalse(result["promoted"])
            self.assertFalse(result["completeChromiumNotice"])
            self.assertFalse(report["spdxDatasetIsBrowserComponent"])
            for filename in ("MIT.json-data", "MIT.license-data", "tag-ref.json-data", "khronos-header-notice.txt"):
                path = output / filename
                original = path.read_bytes()
                path.write_bytes(original + b" ")
                with self.subTest(filename=filename), self.assertRaises(ValueError):
                    self.verified(source, seal)
                path.write_bytes(original)
            (output / "khronos-license-inventory.json").write_bytes(b"{}")
            with self.assertRaisesRegex(ValueError, "receipt SHA256"):
                self.verified(source, seal)

    def test_receipt_cannot_claim_other_revision_full_notice_or_promoted(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-khronos-") as name:
            source = Path(name)
            output, report, _ = self.collected(source)
            for field, value in (("spdxRevision", "0" * 40), ("spdxRepository", "https://foreign"),
                                 ("browserSourceCommit", "0" * 40), ("completeChromiumNotice", True),
                                 ("promoted", True), ("spdxDatasetIsBrowserComponent", True), ("spdxHeaders", 8)):
                data = encoded({**report, field: value})
                (output / "khronos-license-inventory.json").write_bytes(data)
                with self.subTest(field=field), self.assertRaisesRegex(ValueError, "non-promoted"):
                    self.verified(source, hashlib.sha256(data).hexdigest())

    def test_failed_collection_retains_raw_and_incomplete_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-khronos-") as name:
            source = Path(name)
            output = source / "khronos-license-source-02"
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None), patch.dict(self.globals,
                    checked_headers=lambda path: (self.headers, SUBJECT["CONSUMER_SHA"]), fetch=lambda request: b"{}"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](source, output)
            report = json.loads((output / "khronos-license-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertEqual(report["failureType"], "ValueError")
            self.assertFalse(report["records"][0]["verified"])
            self.assertEqual((output / "tag-ref.json-data").read_bytes(), b"{}")
            self.assertFalse((output / "khronos-header-notice.txt").exists())

    def test_total_text_budget_prevents_fragment_and_preserves_failure(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-khronos-") as name:
            source = Path(name)
            with patch.dict(self.globals, MAX_TOTAL=1), self.assertRaisesRegex(ValueError, "selected-text total"):
                self.collected(source)
            report = json.loads((source / "khronos-license-source-01/khronos-license-inventory.json").read_bytes())
            self.assertFalse(report["fullLicenseBindingsComplete"])
            self.assertFalse(report["collectionComplete"])

    def test_escaping_foreign_or_existing_output_never_overwrites_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-khronos-") as name:
            source = Path(name)
            output = source / "khronos-license-source-01"
            output.mkdir()
            (output / "original.txt").write_bytes(b"keep")
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda path: None):
                for invalid in (source / "../khronos-license-source-01", source / "foreign"):
                    with self.assertRaises(ValueError):
                        SUBJECT["collect"](source, invalid)
                with self.assertRaises(FileExistsError):
                    SUBJECT["collect"](source, output)
            self.assertEqual((output / "original.txt").read_bytes(), b"keep")


if __name__ == "__main__":
    unittest.main()
