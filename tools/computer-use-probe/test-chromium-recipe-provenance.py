"""Static recipe helper identity/layout and DEPS fallback, never execution."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-recipe-provenance.py")))


class RecipeProvenanceTests(unittest.TestCase):
    def payloads(self):
        cfg = {"repo_name": "build", "deps": {"depot_tools": {"branch": "refs/heads/main",
               "revision": SUBJECT["DEPOT_REVISION"],
               "url": "https://chromium.googlesource.com/chromium/tools/depot_tools.git"}}}
        return {"build/infra/config/recipes.cfg": json.dumps(cfg).encode(),
                "depot_tools/gclient.py": b"fixed helper bytes", "depot_tools/gclient_eval.py": b"fixed evaluator bytes"}

    def helpers(self, payloads):
        return {name: payloads["depot_tools/" + name] for name in ("gclient.py", "gclient_eval.py")}

    def test_package_helper_revision_and_exact_bytes_match_independent_source(self):
        payloads = self.payloads()
        result = SUBJECT["helper_identity"](payloads, self.helpers(payloads))
        self.assertEqual(result["revision"], SUBJECT["DEPOT_REVISION"])
        self.assertTrue(all(row["exactBytesMatch"] for row in result["helpers"]))

    def test_mutable_or_foreign_dependency_or_changed_helper_rejects(self):
        for variant in ("revision", "url", "helper"):
            payloads = self.payloads()
            helpers = self.helpers(payloads)
            if variant == "helper":
                payloads["depot_tools/gclient.py"] += b"changed"
            else:
                cfg = json.loads(payloads["build/infra/config/recipes.cfg"])
                cfg["deps"]["depot_tools"][variant] = "refs/heads/main" if variant == "revision" else "https://example.test/foreign"
                payloads["build/infra/config/recipes.cfg"] = json.dumps(cfg).encode()
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["helper_identity"](payloads, helpers)

    def test_exact_ast_fallback_and_call_shapes_are_not_executed(self):
        code = b'raise RuntimeError("must never execute collected source")\n' + SUBJECT["FALLBACK"].encode()
        result = SUBJECT["source_shape"](code, SUBJECT["FALLBACK"])
        self.assertEqual(result["startLine"], 2)
        self.assertGreater(result["endLine"], 2)

    def test_changed_or_duplicate_fallback_shape_rejects(self):
        for code in (SUBJECT["FALLBACK"].replace('"DEPS"', '"OTHER"'), SUBJECT["FALLBACK"] * 2):
            with self.subTest(code=code[:40]), self.assertRaises(ValueError):
                SUBJECT["source_shape"](code.encode(), SUBJECT["FALLBACK"])

    def log(self, windows=False):
        root = r"C:\b\s\w\packages\cipd\exe\depot_tools" if windows else "/b/s/w/packages/cipd/exe/depot_tools"
        suffix = "\\recipes\\recipe_modules\\bot_update\\resources" if windows else "/recipes/recipe_modules/bot_update/resources"
        separator = "\\" if windows else "/"
        return ("DEPOT_TOOLS_DIR: " + repr(root) + "\nTHIS_DIR: " + repr(root + suffix)
                + "\ntime ===Running python -u " + root + separator + "gclient.py sync --verbose ===\n").encode()

    def test_original_windows_and_posix_log_layout_is_data_only(self):
        for windows in (False, True):
            result = SUBJECT["log_layout"](self.log(windows))
            self.assertTrue(result["packageRelativeLayoutMatches"])
            self.assertFalse(result["logExecutedByInspector"])
            self.assertFalse(result["builderFilesystemMeasured"])

    def test_foreign_root_command_missing_or_duplicate_directory_rejects(self):
        original = self.log()
        for value in (original.replace(b"packages/cipd/exe", b"checkout/third_party"),
                      original.replace(b"gclient.py sync", b"gclient.py status"),
                      original + original, original.replace(b"THIS_DIR:", b"NOT_THIS_DIR:")):
            with self.subTest(value=value[:40]), self.assertRaises(ValueError):
                SUBJECT["log_layout"](value)

    def test_source_literal_is_parsed_without_function_execution(self):
        malicious = self.log().replace(b"DEPOT_TOOLS_DIR: '/b/s/w/packages/cipd/exe/depot_tools'",
                                      b"DEPOT_TOOLS_DIR: __import__('os').system('never execute')")
        with self.assertRaises(ValueError):
            SUBJECT["log_layout"](malicious)

    def test_escaping_foreign_existing_output_reject_before_source_reads(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recipe-provenance-") as name:
            root = Path(name)
            for output in (root / "foreign", root / "../recipe-provenance-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](root, output)
            output = root / "recipe-provenance-01.json"
            output.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
