"""Only original same-build allowlisted plain logs, never executed commands."""
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-archive-logs.py")))
BUILD_TESTS = runpy.run_path(str(Path(__file__).with_name("test-chromium-archive-builds.py")))


class ArchiveLogTests(unittest.TestCase):
    def build(self, target, profile="core"):
        build = BUILD_TESTS["ArchiveBuildTests"]().build(target)
        build["steps"] = []
        for key, _, _ in SUBJECT["profile_logs"](profile):
            request = SUBJECT["expected_request"](target, key)
            steps = [step for step in build["steps"] if step["name"] == request["step"]]
            if not steps:
                build["steps"].append({"name": request["step"], "status": "SUCCESS", "logs": []})
                steps = [build["steps"][-1]]
            steps[0]["logs"].append({"name": request["log"], "viewUrl": request["url"][:-11]})
        return build

    def test_four_targets_four_fixed_original_raw_log_paths(self):
        for target in SUBJECT["BUILDS"]["ARCHIVES"]:
            requests = SUBJECT["requests_for"](target, self.build(target))
            self.assertEqual(len(requests), 4)
            self.assertEqual([row["key"] for row in requests], ["gclient", "generate", "gn", "upload"])
            self.assertTrue(all(row["url"].endswith("?format=raw") for row in requests))
        with self.assertRaises(ValueError):
            SUBJECT["expected_request"]("linux-x64", "arbitrary")

    def test_checkout_profile_has_exact_three_logs_without_changing_original_core(self):
        for target in SUBJECT["BUILDS"]["ARCHIVES"]:
            requests = SUBJECT["requests_for"](target, self.build(target, "checkout"), "checkout")
            self.assertEqual([row["key"] for row in requests], ["manifest", "checkout", "bootstrap"])
            self.assertTrue(all(row["url"].endswith("?format=raw") for row in requests))
            self.assertEqual(len(SUBJECT["requests_for"](target, self.build(target))), 4)

    def test_unknown_or_cross_profile_output_rejected_before_source_or_network(self):
        root = Path("foreign")
        for output, profile in ((root / "archive-checkout-logs-01", "core"),
                                (root / "archive-build-logs-01", "checkout"),
                                (root / "archive-checkout-logs-01", "arbitrary")):
            with self.subTest(profile=profile), self.assertRaises(ValueError):
                SUBJECT["collect"](root, output, profile)

    def test_checkout_collection_is_twelve_original_data_files_never_execution(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-checkout-logs-") as name:
            root = Path(name)
            builds = {target: self.build(target, "checkout") for target in SUBJECT["BUILDS"]["ARCHIVES"]}
            with patch.dict(SUBJECT["collect"].__globals__, checked_builds=lambda _: (builds, "fixture"),
                            fetch_log=lambda _: (b"original log\n", "text/plain")):
                report = SUBJECT["collect"](root, root / "archive-checkout-logs-01", "checkout")
            self.assertEqual(report["profile"], "checkout")
            self.assertEqual(len(report["logs"]), 12)
            self.assertTrue(report["collectionComplete"])
            self.assertFalse(report["sourceFunctionsExecuted"])
            self.assertFalse(report["promoted"])

    def test_empty_checkout_stdout_preserved_without_relaxing_config_or_manifest(self):
        self.assertEqual(SUBJECT["checked_log"](b"", "text/plain", allow_empty=True), "")
        with self.assertRaises(ValueError):
            SUBJECT["checked_log"](b"", "text/plain")
        with self.assertRaises(ValueError):
            SUBJECT["checked_log"](b"", "text/html", allow_empty=True)
        with tempfile.TemporaryDirectory(prefix="grok-cu-checkout-empty-") as name:
            root = Path(name)
            builds = {"linux-x64": self.build("linux-x64", "checkout")}
            with patch.dict(SUBJECT["collect"].__globals__, checked_builds=lambda _: (builds, "fixture"),
                            fetch_log=lambda request: (b"" if request["key"] == "checkout" else b"data\n", "text/plain")):
                report = SUBJECT["collect"](root, root / "archive-checkout-logs-01", "checkout")
            self.assertTrue(report["collectionComplete"])
            self.assertEqual(report["logs"][1]["bytes"], 0)
            self.assertEqual((root / "archive-checkout-logs-01/linux-x64-checkout.log-data").read_bytes(), b"")

    def test_over_budget_checkout_keeps_bounded_prefix_and_explicit_incompleteness(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-checkout-prefix-") as name:
            root = Path(name)
            builds = {"linux-x64": self.build("linux-x64", "checkout")}
            with patch.dict(SUBJECT["collect"].__globals__, checked_builds=lambda _: (builds, "fixture"),
                            fetch_log=lambda request: (b"x" * (SUBJECT["MAX_LOG_BYTES"] + 1)
                                                      if request["key"] == "checkout" else b"data\n", "text/plain")):
                report = SUBJECT["collect"](root, root / "archive-checkout-logs-01", "checkout")
            self.assertTrue(report["collectionComplete"])
            self.assertFalse(report["fullLogsComplete"])
            self.assertTrue(report["logs"][1]["truncated"])
            self.assertEqual(report["logs"][1]["bytes"], SUBJECT["MAX_LOG_BYTES"])
            self.assertEqual((root / "archive-checkout-logs-01/linux-x64-checkout.log-data").stat().st_size,
                             SUBJECT["MAX_LOG_BYTES"])

    def test_foreign_id_missing_duplicate_failed_or_foreign_log_descriptor_reject(self):
        for variant in ("id", "missing", "duplicate", "status", "url"):
            build = self.build("linux-x64")
            if variant == "id":
                build["id"] = "8668152676980535314"
            elif variant == "missing":
                build["steps"] = build["steps"][1:]
            elif variant == "duplicate":
                build["steps"].append(copy.deepcopy(build["steps"][0]))
            elif variant == "status":
                build["steps"][0]["status"] = "FAILURE"
            else:
                build["steps"][0]["logs"][0]["viewUrl"] = "https://other.example/log"
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                SUBJECT["requests_for"]("linux-x64", build)

    def test_forged_same_build_log_path_or_unknown_fields_refused_before_network(self):
        request = SUBJECT["expected_request"]("linux-x64", "gn")
        for field, value in (("url", request["url"].replace("read_gn_logs.txt/gn_logs.txt", "private/other")),
                             ("url", request["url"] + "&redirect=other"), ("log", "other"),
                             ("buildId", "8668152676980535314"), ("key", None), ("extra", "other")):
            with self.subTest(field=field), self.assertRaises(ValueError):
                SUBJECT["fetch_log"]({**request, field: value})

    def test_only_bounded_plain_utf8_logs_never_html_binary_login(self):
        self.assertEqual(SUBJECT["checked_log"](b"raw log\n", "text/plain; charset=utf-8"), "raw log\n")
        for contents, content_type in ((b"", "text/plain"), (b"\0", "text/plain"), (b"\x91", "text/plain"),
                                       (b"<html>login</html>", "text/plain"), (b"raw", "text/html"),
                                       (b"raw", "text/plain-other"), (b"raw", None),
                                       (b"x" * (SUBJECT["MAX_LOG_BYTES"] + 1), "text/plain")):
            with self.subTest(content_type=content_type, length=len(contents)), self.assertRaises((ValueError, UnicodeError)):
                SUBJECT["checked_log"](contents, content_type)

    def test_original_build_raw_and_sealed_summary_reverified_before_logs(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-logs-") as name:
            root = Path(name)
            output = root / "archive-build-records-01"
            with patch.dict(SUBJECT["BUILDS"]["collect"].__globals__, checked_search=lambda _: SUBJECT["BUILDS"]["SEARCH_SHA"],
                            fetch_record=lambda target: json.dumps(self.build(target)).encode()):
                SUBJECT["BUILDS"]["collect"](root, output)
            sha = hashlib.sha256((output / "archive-build-inventory.json").read_bytes()).hexdigest()
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                    patch.dict(SUBJECT["checked_builds"].__globals__, BUILD_SHA=sha):
                builds, actual = SUBJECT["checked_builds"](root)
                self.assertEqual(actual, sha)
                self.assertEqual(len(builds), 4)
                path = output / "linux-x64.public-data"
                path.write_bytes(path.read_bytes() + b" ")
                with self.assertRaisesRegex(ValueError, "raw bytes changed"):
                    SUBJECT["checked_builds"](root)

    def test_existing_evidence_foreign_source_and_escaping_output_refused(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-logs-") as name:
            root = Path(name)
            output = root / "archive-build-logs-01"
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, output)
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "../archive-build-logs-01")

    def test_complete_collection_preserves_raw_bytes_without_execution_or_promotion(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-logs-") as name:
            root = Path(name)
            builds = {target: self.build(target) for target in SUBJECT["BUILDS"]["ARCHIVES"]}
            with patch.dict(SUBJECT["collect"].__globals__, checked_builds=lambda _: (builds, "fixture"),
                            fetch_log=lambda _: (b"raw command text: do not execute\n", "text/plain")):
                result = SUBJECT["collect"](root, root / "archive-build-logs-01")
            self.assertTrue(result["collectionComplete"])
            self.assertEqual(len(result["logs"]), 16)
            self.assertTrue(all(row["verified"] for row in result["logs"]))
            self.assertFalse(result["sourceFunctionsExecuted"])
            self.assertFalse(result["promoted"])
            self.assertEqual((root / "archive-build-logs-01/linux-x64-gn.log-data").read_bytes(),
                             b"raw command text: do not execute\n")

    def test_late_failure_or_login_content_retained_without_false_success(self):
        for invalid in (False, True):
            with tempfile.TemporaryDirectory(prefix="grok-cu-archive-logs-") as name:
                root = Path(name)
                calls = []
                builds = {"linux-x64": self.build("linux-x64")}

                def fetch(request):
                    calls.append(request)
                    if len(calls) == 2:
                        if invalid:
                            return b"<html>login</html>", "text/html"
                        raise RuntimeError("controlled failure")
                    return b"raw log\n", "text/plain"

                with patch.dict(SUBJECT["collect"].__globals__, checked_builds=lambda _: (builds, "fixture"), fetch_log=fetch), \
                        self.assertRaises((ValueError, RuntimeError)):
                    SUBJECT["collect"](root, root / "archive-build-logs-01")
                result = json.loads((root / "archive-build-logs-01/archive-log-inventory.json").read_bytes())
                self.assertFalse(result["collectionComplete"])
                self.assertEqual(len(result["logs"]), 2 if invalid else 1)
                if invalid:
                    self.assertFalse(result["logs"][-1]["verified"])
                    self.assertEqual((root / "archive-build-logs-01/linux-x64-generate.log-data").read_bytes(),
                                     b"<html>login</html>")


if __name__ == "__main__":
    unittest.main()
