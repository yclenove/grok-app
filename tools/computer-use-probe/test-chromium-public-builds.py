"""Read-only fixed-commit metadata, not public CI success as shipping proof."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-public-builds.py")))


class PublicBuildTests(unittest.TestCase):
    def build(self, identity="8668150172484051841"):
        return {"id": identity, "builder": {"project": "chromium", "bucket": "ci", "builder": "Linux Builder"},
                "status": "SUCCESS"}

    def page(self, builds=None, token=""):
        return json.dumps({"builds": [self.build()] if builds is None else builds, "nextPageToken": token}).encode()

    def test_fixed_read_only_search_body_no_scheduling_or_arbitrary_revision(self):
        body = SUBJECT["search_body"]("opaque")
        self.assertEqual(body["pageToken"], "opaque")
        self.assertEqual(body["pageSize"], 100)
        self.assertEqual(body["predicate"]["builder"], {"project": "chromium", "bucket": "ci"})
        self.assertTrue(body["predicate"]["tags"][0]["value"].endswith(SUBJECT["INPUTS"]["TREE"]["COMMIT"]))

    def test_redirect_is_rejected_before_any_other_endpoint_request(self):
        with self.assertRaisesRegex(ValueError, "may not follow"):
            SUBJECT["NoRedirect"]().redirect_request(None, None, 302, "redirect", {}, "https://other.example/")

    def test_valid_json_and_exact_xssi_prefixes_are_data_only(self):
        for prefix in (b"", b")]}'\n", b")]}'\r\n"):
            builds, token = SUBJECT["public_page"](prefix + self.page())
            self.assertEqual(builds[0]["status"], "SUCCESS")
            self.assertEqual(token, "")

    def test_truncated_xssi_arbitrary_preamble_unknown_fields_and_nonobject_reject(self):
        for contents in (b")]}'", b"garbage" + self.page(), b"[]", b'{"scheduleBuild":{}}', b""):
            with self.subTest(contents=contents), self.assertRaises((ValueError, UnicodeError)):
                SUBJECT["public_page"](contents)

    def test_foreign_project_bucket_duplicate_or_nonnumeric_ids_reject(self):
        for build in ({**self.build(), "id": "../escape"},
                      {**self.build(), "builder": {"project": "private", "bucket": "ci", "builder": "Linux Builder"}},
                      {**self.build(), "builder": {"project": "chromium", "bucket": "try", "builder": "Linux Builder"}}):
            with self.subTest(build=build), self.assertRaises(ValueError):
                SUBJECT["public_page"](self.page([build]))
        with self.assertRaises(ValueError):
            SUBJECT["public_page"](self.page([self.build(), self.build()]))

    def test_page_and_opaque_token_budgets_are_enforced_not_interpreted(self):
        with self.assertRaises(ValueError):
            SUBJECT["public_page"](b"x" * (SUBJECT["MAX_PAGE_BYTES"] + 1))
        with self.assertRaises(ValueError):
            SUBJECT["public_page"](self.page([self.build(str(8000000000000000000 + index)) for index in range(101)]))
        for token in (None, "a" * 16385, "a\0b", "\u4e2d"):
            with self.subTest(token=token), self.assertRaises(ValueError):
                SUBJECT["search_body"](token)

    def test_available_input_revision_host_and_project_must_match_not_just_query_tag(self):
        commit = {"host": "chromium.googlesource.com", "project": "chromium/src",
                  "id": SUBJECT["INPUTS"]["TREE"]["COMMIT"]}
        SUBJECT["public_page"](self.page([{**self.build(), "input": {"gitilesCommit": commit}}]))
        for key, value in (("host", "other.example"), ("project", "chromium/other"), ("id", "a" * 40)):
            build = {**self.build(), "input": {"gitilesCommit": {**commit, key: value}}}
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["public_page"](self.page([build]))
        with self.assertRaises(ValueError):
            SUBJECT["public_page"](self.page([{**self.build(), "input": None}]))

    def test_foreign_source_and_existing_evidence_reject_before_network(self):
        with self.assertRaises(ValueError):
            SUBJECT["collect"](Path("foreign"), Path("foreign/public-build-search-01"))
        with tempfile.TemporaryDirectory(prefix="grok-cu-public-builds-") as name:
            root = Path(name)
            output = root / "public-build-search-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_paged_collection_success_preserves_raw_bytes_but_never_claims_publisher(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-public-builds-") as name:
            root = Path(name)
            output = root / "public-build-search-01"
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                    patch.dict(SUBJECT["collect"].__globals__, fetch_page=lambda _: self.page()):
                report = SUBJECT["collect"](root, output)
            self.assertTrue(report["collectionComplete"])
            self.assertFalse(report["publisherAssociationProven"])
            self.assertFalse(report["builderInputsCollected"])
            self.assertEqual((output / "page-01.public-data").read_bytes(), self.page())

    def test_late_failure_or_cyclic_pagination_preserves_partial_evidence(self):
        for cyclic in (False, True):
            with tempfile.TemporaryDirectory(prefix="grok-cu-public-builds-") as name:
                root = Path(name)
                output = root / "public-build-search-01"
                pages = iter([self.page(token="repeat"), self.page([self.build("8668150172484051842")], "repeat")])
                calls = []

                def fetch(token):
                    calls.append(token)
                    if len(calls) > 1 and not cyclic:
                        raise RuntimeError("controlled fixture failure")
                    return next(pages)

                with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                        patch.dict(SUBJECT["collect"].__globals__, fetch_page=fetch), \
                        self.assertRaises((ValueError, RuntimeError)):
                    SUBJECT["collect"](root, output)
                report = json.loads((output / "public-build-inventory.json").read_bytes())
                self.assertFalse(report["collectionComplete"])
                self.assertEqual(len(report["pages"]), 1)
                self.assertEqual(len(report["builds"]), 1)
                self.assertTrue((output / "page-01.public-data").exists())


if __name__ == "__main__":
    unittest.main()
