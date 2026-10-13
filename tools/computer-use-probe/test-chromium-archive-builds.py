"""Archive metadata is exact source/path evidence, not binary/GN/license proof."""
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-archive-builds.py")))


class ArchiveBuildTests(unittest.TestCase):
    def commit(self, output=False):
        result = {"host": "chromium.googlesource.com", "project": "chromium/src",
                  "id": SUBJECT["INPUTS"]["TREE"]["COMMIT"]}
        if output:
            result["position"] = SUBJECT["POSITION"]
        return result

    def build(self, target="linux-x64"):
        builder, identity, platform, filename = SUBJECT["archive"](target)
        relative = platform + "/1716663/" + filename
        step = "Generic Archiving Steps|gsutil upload " + relative
        properties = {"got_revision": self.commit()["id"], "got_revision_cp": "refs/heads/main@{#1716663}",
                      "gsutil_urls": {step: "gs://chromium-browser-snapshots/" + relative}}
        for component in ("angle", "swiftshader", "v8", "webrtc"):
            properties["got_" + component + "_revision"] = SUBJECT["INPUTS"]["EXTERNAL"]["PINS"][component][2]
        return {"id": identity, "builder": {"project": "chromium", "bucket": "ci", "builder": builder},
                "status": "SUCCESS", "input": {"gitilesCommit": self.commit(), "properties": {}},
                "output": {"gitilesCommit": self.commit(True), "properties": properties},
                "steps": [{"name": step, "status": "SUCCESS"}],
                "tags": [{"key": "buildset", "value": "commit/gitiles/chromium.googlesource.com/chromium/src/+/"
                          + self.commit()["id"]}]}

    def inspect(self, build, target="linux-x64"):
        return SUBJECT["inspect_record"](target, json.dumps(build).encode())

    def log(self, identity=None):
        identity = identity or SUBJECT["archive"]("linux-x64")[1]
        return {"name": "config", "viewUrl": "https://logs.chromium.org/logs/chromium/buildbucket/cr-buildbucket/"
                + identity + "/+/u/gclient_config/config"}

    def search(self, root):
        directory = root / "public-build-search-01"
        directory.mkdir()
        builds = [self.build(target) for target in SUBJECT["ARCHIVES"]]
        for index in range(357):
            row = copy.deepcopy(builds[0])
            row["id"] = str(8000000000000000000 + index)
            row["builder"]["builder"] = "Other Builder"
            builds.append(row)
        pages = []
        for index in range(4):
            name = "page-" + str(index + 1).zfill(2) + ".public-data"
            rows = builds[index * 100:(index + 1) * 100]
            data = json.dumps({"builds": rows, "nextPageToken": str(index + 1) if index < 3 else ""}).encode()
            (directory / name).write_bytes(data)
            pages.append({"evidenceFile": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                          "builds": len(rows)})
        report = {"sourceCommit": self.commit()["id"], "collectionComplete": True,
                  "readOnlyRpc": SUBJECT["SEARCH"]["ENDPOINT"], "query": SUBJECT["SEARCH"]["search_body"](""),
                  "nextPageToken": "", "pages": pages, "builds": builds}
        path = directory / "public-build-inventory.json"
        SUBJECT["INPUTS"]["EXTERNAL"]["write_report"](path, report)
        return report, hashlib.sha256(path.read_bytes()).hexdigest()

    def test_only_compiled_targets_and_fixed_read_only_rpc_fields(self):
        for target, row in SUBJECT["ARCHIVES"].items():
            self.assertEqual(SUBJECT["get_body"](target), {"id": row[1], "fields": "id,builder,status,input,output,steps,tags"})
        for target in (None, [], "../escape", "8668152676980535313"):
            with self.subTest(target=target), self.assertRaises(ValueError):
                SUBJECT["get_body"](target)

    def test_four_targets_and_xssi_preserve_uncertainty(self):
        for target in SUBJECT["ARCHIVES"]:
            for prefix in (b"", b")]}'\n", b")]}'\r\n"):
                result = SUBJECT["inspect_record"](target, prefix + json.dumps(self.build(target)).encode())
                self.assertTrue(result["publisherPathVerified"])
                for flag in ("gcsGenerationAssociationProven", "downloadedZipBytesAssociationProven",
                             "actualGnArgsVerified", "actualGclientVarsVerified", "promoted"):
                    self.assertIs(result[flag], False)

    def test_wrong_identity_builder_status_missing_and_unexpected_fields_reject(self):
        for field, value in (("id", "8668152676980535314"), ("status", "FAILURE"),
                             ("builder", {"project": "chromium", "bucket": "try", "builder": "linux-archive-rel"}),
                             ("input", [])):
            build = self.build()
            build[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.inspect(build)
        for remove in (True, False):
            build = self.build()
            if remove:
                del build["steps"]
            else:
                build["scheduleBuild"] = {}
            with self.assertRaises(ValueError):
                self.inspect(build)

    def test_input_and_output_exact_commit_not_query_tag_only(self):
        for part in ("input", "output"):
            for field, value in (("host", "other.example"), ("project", "chromium/other"), ("id", "a" * 40)):
                build = self.build()
                build[part]["gitilesCommit"][field] = value
                with self.subTest(part=part, field=field), self.assertRaises(ValueError):
                    self.inspect(build)
        for position in (None, True, "1716663", 1716664):
            build = self.build()
            build["output"]["gitilesCommit"]["position"] = position
            with self.subTest(position=position), self.assertRaises(ValueError):
                self.inspect(build)

    def test_output_revision_properties_must_agree_with_actual_commit(self):
        for field in ("got_revision", "got_revision_cp"):
            build = self.build()
            build["output"]["properties"][field] = "other"
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.inspect(build)

    def test_each_collected_output_component_pin_must_match_fixed_parent(self):
        for component in ("angle", "swiftshader", "v8", "webrtc"):
            build = self.build()
            build["output"]["properties"]["got_" + component + "_revision"] = "b" * 40
            with self.subTest(component=component), self.assertRaisesRegex(ValueError, "component pin"):
                self.inspect(build)

    def test_exact_upload_path_and_successful_actual_step_both_required(self):
        for variant in ("url", "name", "status", "absent"):
            build = self.build()
            if variant == "url":
                key = next(iter(build["output"]["properties"]["gsutil_urls"]))
                build["output"]["properties"]["gsutil_urls"][key] += "?generation=other"
            elif variant == "absent":
                build["steps"] = [{"name": "other", "status": "SUCCESS"}]
            else:
                build["steps"][0][variant] = "other"
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                self.inspect(build)

    def test_inert_same_build_log_descriptors_only_never_fetch(self):
        build = self.build()
        build["steps"].append({"name": "gclient config", "status": "SUCCESS", "logs": [self.log()]})
        result = self.inspect(build)
        self.assertFalse(result["selectedSteps"][1]["logs"][0]["fetched"])
        urls = (self.log("8668152676980535314")["viewUrl"], "https://other.example/config",
                self.log()["viewUrl"] + "?redirect=other", self.log()["viewUrl"] + "/../escape",
                self.log()["viewUrl"].replace("https:", "http:"))
        for url in urls:
            build["steps"][1]["logs"][0]["viewUrl"] = url
            with self.subTest(url=url), self.assertRaises(ValueError):
                self.inspect(build)

    def test_malformed_nonjson_binary_and_oversize_records_reject(self):
        for data in (b"", b"[]", b")]}'", b"garbage", b"\x91", b"x" * (SUBJECT["MAX_RECORD_BYTES"] + 1)):
            with self.subTest(length=len(data)), self.assertRaises((ValueError, UnicodeError)):
                SUBJECT["inspect_record"]("linux-x64", data)

    def test_duplicate_steps_or_buildset_tags_and_step_budget_reject(self):
        for variant in ("steps", "tags", "budget"):
            build = self.build()
            if variant == "budget":
                build["steps"] *= 2001
            else:
                build[variant] *= 2
            with self.subTest(variant=variant), self.assertRaises(ValueError):
                self.inspect(build)

    def test_search_seal_all_raw_pages_and_full_inventory_are_reverified(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-build-") as name:
            root = Path(name)
            _, sha = self.search(root)
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                    patch.dict(SUBJECT["checked_search"].__globals__, SEARCH_SHA=sha):
                self.assertEqual(SUBJECT["checked_search"](root), sha)
                page = root / "public-build-search-01/page-02.public-data"
                page.write_bytes(page.read_bytes() + b" ")
                with self.assertRaisesRegex(ValueError, "identity changed"):
                    SUBJECT["checked_search"](root)

    def test_foreign_search_seal_missing_archive_and_inventory_disagreement_reject(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-build-") as name:
            root = Path(name)
            report, _ = self.search(root)
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), self.assertRaisesRegex(ValueError, "SHA256"):
                SUBJECT["checked_search"](root)
            report["builds"] = report["builds"][1:]
            path = root / "public-build-search-01/public-build-inventory.json"
            path.write_text(json.dumps(report), encoding="utf-8")
            sha = hashlib.sha256(path.read_bytes()).hexdigest()
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _: None), \
                    patch.dict(SUBJECT["checked_search"].__globals__, SEARCH_SHA=sha), \
                    self.assertRaisesRegex(ValueError, "inventory must agree"):
                SUBJECT["checked_search"](root)

    def test_existing_output_or_foreign_source_refused_before_network(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-build-") as name:
            root = Path(name)
            output = root / "archive-build-records-01"
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, output)
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "../archive-build-records-01")

    def test_complete_collection_saves_original_bytes_not_full_stdout_or_shipping_proof(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-archive-build-") as name:
            root = Path(name)
            output = root / "archive-build-records-01"
            data = {target: json.dumps(self.build(target)).encode() for target in SUBJECT["ARCHIVES"]}
            with patch.dict(SUBJECT["collect"].__globals__, checked_search=lambda _: "fixture", fetch_record=data.get):
                result = SUBJECT["collect"](root, output)
            self.assertTrue(result["collectionComplete"])
            self.assertEqual(len(result["records"]), 4)
            self.assertFalse(result["logsFetched"])
            self.assertFalse(result["promoted"])
            for row in result["records"]:
                self.assertTrue(row["verified"])
                self.assertEqual((output / row["evidenceFile"]).read_bytes(), data[row["target"]])

    def test_late_network_or_record_failure_retains_completed_and_invalid_evidence(self):
        for invalid in (False, True):
            with tempfile.TemporaryDirectory(prefix="grok-cu-archive-build-") as name:
                root = Path(name)
                output = root / "archive-build-records-01"
                calls = []

                def fetch(target):
                    calls.append(target)
                    if len(calls) == 2:
                        if invalid:
                            return b"invalid but bounded raw evidence"
                        raise RuntimeError("controlled network failure")
                    return json.dumps(self.build(target)).encode()

                with patch.dict(SUBJECT["collect"].__globals__, checked_search=lambda _: "fixture", fetch_record=fetch), \
                        self.assertRaises((ValueError, RuntimeError)):
                    SUBJECT["collect"](root, output)
                result = json.loads((output / "archive-build-inventory.json").read_bytes())
                self.assertFalse(result["collectionComplete"])
                self.assertEqual(len(result["summaries"]), 1)
                self.assertEqual(len(result["records"]), 2 if invalid else 1)
                if invalid:
                    self.assertFalse(result["records"][-1]["verified"])
                    self.assertEqual((output / "linux-x64.public-data").read_bytes(), b"invalid but bounded raw evidence")


if __name__ == "__main__":
    unittest.main()
