"""Fixed recipe identity, immutable requests, bounded data and late failures."""
import copy
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-recipe-instance.py")))


class RecipeInstanceTests(unittest.TestCase):
    def instance(self):
        return {"package": SUBJECT["PACKAGE"], "instance": SUBJECT["object_ref"]()}

    def test_original_instance_decodes_to_exact_sha256_not_mutable_ref(self):
        instance = SUBJECT["object_ref"]()
        self.assertEqual(instance["hashAlgo"], "SHA256")
        self.assertEqual(len(instance["hexDigest"]), 64)
        self.assertEqual(SUBJECT["requests"]()[0]["body"]["version"], SUBJECT["VERSION"])
        self.assertNotIn("refs/heads/main", json.dumps(SUBJECT["requests"]()))

    def test_foreign_endpoint_method_package_or_version_reject_before_network(self):
        for key, value in (("url", "https://example.test/RPC"), ("key", "register"),
                           ("body", {"package": SUBJECT["PACKAGE"], "version": "refs/heads/main"})):
            request = {**SUBJECT["requests"]()[0], key: value}
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["fetch"](request)

    def test_resolve_and_describe_require_original_package_and_digest(self):
        SUBJECT["parse_response"]("resolve", json.dumps(self.instance()).encode())
        SUBJECT["parse_response"]("describe", b")]}'\n" + json.dumps({"instance": self.instance()}).encode())
        for value in ({**self.instance(), "package": "foreign"},
                      {**self.instance(), "instance": {"hashAlgo": "SHA1", "hexDigest": "a" * 40}}, None):
            for key in ("resolve", "describe"):
                contents = json.dumps(value if key == "resolve" else {"instance": value}).encode()
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    SUBJECT["parse_response"](key, contents)

    def test_response_limits_html_json_shape_and_unknown_rpc_reject(self):
        for key, contents in (("resolve", b""), ("resolve", b"x" * (SUBJECT["MAX_RESPONSE"] + 1)),
                              ("resolve", b"<html>login</html>"), ("describe", b"[]"), ("register", b"{}")):
            with self.subTest(key=key, size=len(contents)), self.assertRaises((ValueError, json.JSONDecodeError)):
                SUBJECT["parse_response"](key, contents)
        for rows in ({}, [None], [{}] * 1025):
            with self.subTest(rows_type=type(rows)), self.assertRaises(ValueError):
                SUBJECT["parse_response"]("describe", json.dumps({"instance": self.instance(), "tags": rows}).encode())

    def test_all_four_original_bootstraps_reproduced_before_network(self):
        report = {"targets": [{"target": target, "bootstrap": {"recipe": "chromium",
                  "recipeExe": {"cmd": ["luciexe"], "cipd": {"server": SUBJECT["SERVER"],
                    "package": SUBJECT["PACKAGE"], "actual_version": SUBJECT["VERSION"]}}}}
                  for target in SUBJECT["CHECKOUTS"]["BUILDS"]["ARCHIVES"]]}
        for variant in ("valid", "foreign", "missing", "changed"):
            value = copy.deepcopy(report)
            if variant == "foreign":
                value["targets"][-1]["bootstrap"]["recipeExe"]["cipd"]["actual_version"] = "refs/heads/main"
            elif variant == "missing":
                value["targets"].pop()
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None,
                            receipt=lambda *_: (value, "original")), \
                    patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda _: {} if variant == "changed" else value):
                if variant == "valid":
                    self.assertEqual(SUBJECT["checked_inputs"](Path("fixture")), "original")
                else:
                    with self.subTest(variant=variant), self.assertRaises(ValueError):
                        SUBJECT["checked_inputs"](Path("fixture"))

    def test_escaping_foreign_or_existing_output_reject(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recipe-") as name:
            root = Path(name)
            for output in (root / "foreign", root / "../recipe-instance-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, output)
            output = root / "recipe-instance-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_complete_collection_and_late_bad_digest_retain_raw_without_promotion(self):
        for fail in (False, True):
            def fetch(request):
                if request["key"] == "resolve":
                    return json.dumps(self.instance()).encode()
                return json.dumps({"instance": {} if fail else self.instance(), "tags": [{"tag": "git_revision:fixture"}]}).encode()
            with self.subTest(fail=fail), tempfile.TemporaryDirectory(prefix="grok-cu-recipe-") as name:
                root = Path(name)
                with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda _: "original", fetch=fetch):
                    if fail:
                        with self.assertRaises(ValueError):
                            SUBJECT["collect"](root, root / "recipe-instance-01")
                    else:
                        SUBJECT["collect"](root, root / "recipe-instance-01")
                output = root / "recipe-instance-01"
                report = json.loads((output / "recipe-instance-inventory.json").read_bytes())
                self.assertEqual(report["collectionComplete"], not fail)
                self.assertEqual(len(report["responses"]), 2)
                self.assertTrue((output / "describe.public-data").is_file())
                self.assertFalse(report["packageDownloaded"])
                self.assertFalse(report["actualGclientHelperRevisionVerified"])
                self.assertFalse(report["promoted"])
                self.assertEqual(report["responses"][-1]["verified"], not fail)


if __name__ == "__main__":
    unittest.main()
