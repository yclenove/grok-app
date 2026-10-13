"""Exact config URLs, independently pinned Git blobs, bounded data/failures."""
import base64
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-builder-config.py")))


class BuilderConfigTests(unittest.TestCase):
    def fixture(self):
        contents = b'{"recipe":"chromium","value":true}\n'
        oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
        return contents, oid

    def test_exact_four_paths_and_commit_not_live_branch(self):
        requests = SUBJECT["requests"]()
        self.assertEqual(len(requests), 4)
        self.assertEqual(len({row["target"] for row in requests}), 4)
        for row in requests:
            self.assertIn("/+/" + SUBJECT["REVISION"] + "/", row["url"])
            self.assertTrue(row["url"].endswith("/properties.json?format=TEXT"))

    def test_foreign_path_commit_or_endpoint_rejects_before_network(self):
        for key, value in (("url", "https://example.test/config"), ("path", "../other"), ("target", "foreign")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["fetch"]({**SUBJECT["requests"]()[0], key: value})

    def test_decoded_config_matches_git_blob_not_only_its_new_local_hash(self):
        contents, oid = self.fixture()
        decoded, config, identity = SUBJECT["decoded_config"](base64.b64encode(contents), oid)
        self.assertEqual(decoded, contents)
        self.assertEqual(config["recipe"], "chromium")
        self.assertEqual(identity["sha256"], hashlib.sha256(contents).hexdigest())
        with self.assertRaises(ValueError):
            SUBJECT["decoded_config"](base64.b64encode(contents + b"changed"), oid)

    def test_invalid_base64_json_shape_and_size_reject(self):
        contents, oid = self.fixture()
        for raw in (b"", b"<html>login</html>", b"x" * (SUBJECT["MAX_RAW"] + 1)):
            with self.subTest(size=len(raw)), self.assertRaises(ValueError):
                SUBJECT["decoded_config"](raw, oid)
        for decoded in (b"[]", b"{}"):
            value_oid = hashlib.sha1(b"blob " + str(len(decoded)).encode() + b"\0" + decoded).hexdigest()
            with self.assertRaises(ValueError):
                SUBJECT["decoded_config"](base64.b64encode(decoded), value_oid)

    def test_bootstrap_differences_are_recorded_without_override_evaluation(self):
        result = SUBJECT["compare_bootstrap"]({"same": 1, "changed": True, "absent": "source"}, {"same": 1, "changed": False})
        self.assertEqual(result["exactValueMatches"], ["same"])
        self.assertEqual(result["keysAbsentFromBootstrap"], ["absent"])
        self.assertEqual(result["valueDifferences"], ["changed"])
        self.assertFalse(result["overrideSemanticsEvaluated"])
        self.assertFalse(result["configExecuted"])
        self.assertEqual(SUBJECT["compare_bootstrap"]({"number": 1}, {"number": True})["valueDifferences"], ["number"])

    def test_late_wrong_blob_preserves_first_source_and_bad_raw(self):
        contents, oid = self.fixture()
        def fetch(request):
            return base64.b64encode(contents if request["target"] == SUBJECT["requests"]()[0]["target"] else b"changed")
        with tempfile.TemporaryDirectory(prefix="grok-cu-builder-config-") as name:
            root = Path(name)
            expected = {row["target"]: oid for row in SUBJECT["requests"]()}
            bootstraps = {key: {"recipe": "chromium", "value": True} for key in expected}
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda _: (expected, bootstraps), fetch=fetch), \
                    self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "builder-config-source-01")
            output = root / "builder-config-source-01"
            report = json.loads((output / "builder-config-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertFalse(report["promoted"])
            self.assertEqual(len(report["records"]), 2)
            self.assertTrue(report["records"][0]["verified"])
            self.assertFalse(report["records"][1]["verified"])
            self.assertTrue((output / report["records"][1]["rawFile"]).is_file())


if __name__ == "__main__":
    unittest.main()
