"""Recipe digest binding, URL confinement, ZIP bounds and preserved failures."""
import hashlib
import io
import json
from pathlib import Path
import runpy
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-recipe-package.py")))


class Response(io.BytesIO):
    def __init__(self, contents, url, length=None):
        super().__init__(contents)
        self.url = url
        self.headers = {} if length is None else {"Content-Length": str(length)}

    def geturl(self):
        return self.url


class Opener:
    def __init__(self, response):
        self.response = response

    def open(self, request, timeout):
        if timeout != 20 or request.get_method() != "GET":
            raise AssertionError("fixed bounded read-only request required")
        return self.response


class RecipePackageTests(unittest.TestCase):
    def test_fixed_request_is_original_digest_not_mutable_version(self):
        self.assertEqual(SUBJECT["url_request"](), {"package": SUBJECT["RECIPE"]["PACKAGE"],
                                                   "instance": SUBJECT["RECIPE"]["object_ref"]()})
        self.assertTrue(SUBJECT["ENDPOINT"].endswith("/GetInstanceURL"))

    def test_signed_storage_url_is_never_arbitrary_redirect_or_local_endpoint(self):
        valid = "https://storage.googleapis.com/public/object?Signature=opaque"
        self.assertEqual(SUBJECT["checked_url"](valid), valid)
        for url in ("http://storage.googleapis.com/x", "https://storage.googleapis.com.evil.test/x",
                    "https://user@storage.googleapis.com/x", "https://storage.googleapis.com:443/x",
                    "https://127.0.0.1/x", valid + "#fragment", valid + "\n", valid + " " * 33000):
            with self.subTest(url=url[:80]), self.assertRaises(ValueError):
                SUBJECT["checked_url"](url)

    def test_url_rpc_requires_bounded_original_json_with_signed_url(self):
        url = "https://storage.googleapis.com/public/fixed"
        self.assertEqual(SUBJECT["parsed_url"](b")]}'\n" + json.dumps({"signedUrl": url}).encode()), url)
        for contents in (b"", b"[]", b"{}", b"<html>login</html>", b"x" * (SUBJECT["MAX_RPC"] + 1)):
            with self.subTest(size=len(contents)), self.assertRaises(ValueError):
                SUBJECT["parsed_url"](contents)

    def test_download_stream_matches_digest_and_does_not_execute_or_extract(self):
        contents = b"fixed inert source fixture"
        url = "https://storage.googleapis.com/public/fixed"
        with tempfile.TemporaryDirectory(prefix="grok-cu-recipe-zip-") as name:
            path = Path(name) / "package.zip-data"
            with patch.dict(SUBJECT["download"].__globals__, opener=lambda: Opener(Response(contents, url, len(contents)))), \
                    patch.dict(SUBJECT["RECIPE"], object_ref=lambda: {"hexDigest": hashlib.sha256(contents).hexdigest()}):
                result = SUBJECT["download"](url, path)
            self.assertEqual(path.read_bytes(), contents)
            self.assertEqual(result["bytes"], len(contents))

    def test_wrong_digest_size_redirect_and_budget_fail_without_losing_raw(self):
        contents, url = b"bad original digest", "https://storage.googleapis.com/public/fixed"
        for variant in ("digest", "size", "redirect", "budget"):
            with self.subTest(variant=variant), tempfile.TemporaryDirectory(prefix="grok-cu-recipe-zip-") as name:
                path = Path(name) / "package.zip-data"
                response = Response(contents, url + "other" if variant == "redirect" else url,
                                    len(contents) + 1 if variant == "size" else None)
                with patch.dict(SUBJECT["download"].__globals__, opener=lambda: Opener(response),
                                MAX_ZIP=4 if variant == "budget" else SUBJECT["MAX_ZIP"]), self.assertRaises(ValueError):
                    SUBJECT["download"](url, path)
                self.assertTrue(path.is_file())
                self.assertLessEqual(path.stat().st_size, 4 if variant == "budget" else len(contents))

    def archive(self, paths):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as archive:
            for path in paths:
                archive.writestr(path, b"fixture")
        stream.seek(0)
        return zipfile.ZipFile(stream)

    def test_zip_paths_metadata_bounds_and_duplicate_entries_reject(self):
        for paths in (("../recipes.cfg",), ("/recipes.cfg",), ("x:recipes.cfg",)):
            with self.subTest(paths=paths), self.archive(paths) as archive, self.assertRaises(ValueError):
                SUBJECT["zip_records"](archive)
        with self.assertWarnsRegex(UserWarning, "Duplicate name"):
            with self.archive(("recipes.cfg", "recipes.cfg")) as archive, self.assertRaises(ValueError):
                SUBJECT["zip_records"](archive)
        # Windows ZipInfo normalizes backslashes on write, so mutate the read
        # record to test the parser rather than a silently normalized fixture.
        with self.archive(("recipes.cfg",)) as archive:
            info = archive.infolist()[0]
            info.filename = info.orig_filename = "x\\recipes.cfg"
            with self.assertRaises(ValueError):
                SUBJECT["zip_records"](archive)
        with self.archive(("recipes.cfg",)) as archive:
            info = archive.infolist()[0]
            info.orig_filename += "\0hidden"
            with self.assertRaises(ValueError):
                SUBJECT["zip_records"](archive)
        for limit in ("MAX_ENTRIES", "MAX_FILE", "MAX_TREE"):
            with self.subTest(limit=limit), self.archive(("recipes.cfg",)) as archive, \
                    patch.dict(SUBJECT["zip_records"].__globals__, {limit: 0}), self.assertRaises(ValueError):
                SUBJECT["zip_records"](archive)

    def test_symlinks_and_special_files_are_not_selected_or_materialized(self):
        with self.archive(("repo/recipes.cfg", "repo/recipe_modules/gclient/api.py", "other/ignore.txt")) as archive:
            rows, size = SUBJECT["zip_records"](archive)
            self.assertEqual(sum(row["selected"] for row in rows), 2)
            self.assertEqual(size, 21)
            archive.infolist()[0].external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.infolist()[1].external_attr = (stat.S_IFIFO | 0o600) << 16
            rows, _ = SUBJECT["zip_records"](archive)
            self.assertEqual([row["kind"] for row in rows[:2]], ["symlink", "other"])
            self.assertFalse(any(row["selected"] for row in rows))

    def test_selected_zip_sources_are_inert_hash_names_not_upstream_paths(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recipe-zip-") as name:
            root = Path(name)
            path = root / "recipe.zip-data"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("repo/build/infra/config/recipes.cfg", b'{"deps": {}}')
                archive.writestr("repo/build/recipes/chromium.py", b"raise RuntimeError('never execute')\n")
                archive.writestr("unselected.txt", b"unselected")
            report = SUBJECT["inspect_zip"](path, root)
            self.assertEqual(len(report["selectedFiles"]), 2)
            self.assertFalse((root / "repo").exists())
            for row in report["selectedFiles"]:
                self.assertEqual(hashlib.sha256((root / row["evidenceFile"]).read_bytes()).hexdigest(), row["sha256"])

    def test_late_invalid_url_retains_raw_and_partial_receipt_without_download(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recipe-zip-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda _: "original",
                            fetch_url=lambda: b'{"signedUrl":"https://example.test/foreign"}'), self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "recipe-package-01")
            output = root / "recipe-package-01"
            self.assertTrue((output / "url.public-data").is_file())
            report = json.loads((output / "recipe-package-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertFalse(report["packageInstalled"])
            self.assertFalse(report["archiveExtracted"])
            self.assertFalse(report["promoted"])
            self.assertFalse((output / "recipe-package.zip-data").exists())


if __name__ == "__main__":
    unittest.main()
