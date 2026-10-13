"""Configuration evidence cannot alias metadata or silently forgive missing sources."""
import base64
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-crashpad-coverage.py")))
PREFIX = SUBJECT["CRASHPAD"]


def identity(contents):
    oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
    return oid, {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}


class CrashpadCoverageTests(unittest.TestCase):
    def fixture(self):
        texts = {
            ".gn": 'buildconfig = "//build/config/BUILDCONFIG.gn"\ndefault_args = {\n  crashpad_dependencies = "chromium"\n}\n',
            PREFIX + "build/crashpad_buildconfig.gni": 'crashpad_is_in_chromium = crashpad_dependencies == "chromium"\n',
            PREFIX + "third_party/lss/BUILD.gn": 'config("lss_config") {\n  if (crashpad_is_in_chromium) {\n    defines = [ "CRASHPAD_LSS_SOURCE_EXTERNAL" ]\n  }\n}\n',
            PREFIX + "third_party/lss/lss.h": '#if defined(CRASHPAD_LSS_SOURCE_EXTERNAL)\n#include "third_party/lss/linux_syscall_support.h"\n#endif\n',
            PREFIX + "third_party/zlib/BUILD.gn": 'if (crashpad_is_in_chromium || crashpad_is_in_fuchsia || crashpad_is_in_dart) {\n  zlib_source = "external"\n}\nif (zlib_source == "external") {\n  group("zlib") {\n    public_configs = [ ":zlib_config" ]\n    public_deps = [ "//third_party/zlib" ]\n  }\n}\n',
            PREFIX + "third_party/zlib/zlib_crashpad.h": '#if defined(CRASHPAD_ZLIB_SOURCE_SYSTEM) || \\\n    defined(CRASHPAD_ZLIB_SOURCE_EXTERNAL)\n#include <zlib.h>\n#endif\n',
        }
        targets = [{"target": name, "buildId": str(number), "gnArgsWritten": {"target_os": os_name, "target_cpu": cpu},
                    "explicitGnArgsVerified": True} for number, (name, (os_name, cpu)) in enumerate(SUBJECT["ARGS"]["PLATFORMS"].items())]
        correspondence = [{"target": row["target"], "status": "exact_repository_and_revision_match" if row["target"] == "linux-x64"
                            else "exact_manifest_entry_absent"} for row in targets]
        return {path: text.encode() for path, text in texts.items()}, targets, correspondence

    def test_root_default_actual_args_and_external_rules_recorded_without_exclusions(self):
        blobs, targets, correspondence = self.fixture()
        original = copy.deepcopy((blobs, targets, correspondence))
        report = SUBJECT["configuration_evidence"](blobs, targets, correspondence)
        self.assertEqual(len(report["rules"]), 8)
        self.assertEqual(len(report["targets"]), 4)
        self.assertEqual(report["sourceLevelMapping"][0]["actualCheckoutMatchTargets"], ["linux-x64"])
        self.assertEqual(report["sourceLevelMapping"][0]["licensePath"], "third_party/lss/LICENSE")
        self.assertEqual(report["sourceLevelMapping"][1]["licensePath"], "third_party/zlib/LICENSE")
        for key in ("effectiveGnMeasured", "metadataPathsAliased", "originalErrorsExcluded"):
            self.assertFalse(report[key])
        for row in report["targets"]:
            self.assertFalse(row["binaryCoverageProven"])
            self.assertFalse(row["effectiveGnMeasured"])
        self.assertEqual((blobs, targets, correspondence), original)

    def test_standalone_default_or_embedded_lss_branch_cannot_be_used_as_chromium(self):
        for path, old, new in ((".gn", b'"chromium"', b'"standalone"'),
                               (PREFIX + "third_party/lss/BUILD.gn", b"CRASHPAD_LSS_SOURCE_EXTERNAL", b"CRASHPAD_LSS_SOURCE_EMBEDDED"),
                               (PREFIX + "third_party/zlib/BUILD.gn", b'"//third_party/zlib"', b'"//other/zlib"')):
            blobs, targets, correspondence = self.fixture()
            blobs[path] = blobs[path].replace(old, new)
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "one exact pinned"):
                SUBJECT["configuration_evidence"](blobs, targets, correspondence)

    def test_recorded_override_requires_explicit_new_review(self):
        for value in ("standalone", "chromium", "fuchsia"):
            blobs, targets, correspondence = self.fixture()
            targets[0]["gnArgsWritten"]["crashpad_dependencies"] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "override"):
                SUBJECT["configuration_evidence"](blobs, targets, correspondence)

    def test_missing_duplicate_target_or_unverified_actual_args_refused(self):
        blobs, targets, correspondence = self.fixture()
        for changed in (targets[:-1], targets + [targets[0]], [targets[0]] * 4):
            with self.subTest(changed=changed), self.assertRaisesRegex(ValueError, "four exact"):
                SUBJECT["configuration_evidence"](blobs, changed, correspondence)
        targets[0]["explicitGnArgsVerified"] = False
        with self.assertRaisesRegex(ValueError, "independently verified"):
            SUBJECT["configuration_evidence"](blobs, targets, correspondence)

    def test_rule_excerpt_records_exact_line_hash_and_rejects_duplicates(self):
        blobs = {"path": b"prefix\nexact rule\nsuffix\n"}
        row = SUBJECT["excerpt"](blobs, "path", "^exact rule$")
        self.assertEqual(row["line"], 2)
        self.assertEqual(row["sourceSha256"], hashlib.sha256(blobs["path"]).hexdigest())
        blobs["path"] += b"exact rule\n"
        with self.assertRaises(ValueError):
            SUBJECT["excerpt"](blobs, "path", "^exact rule$")

    def source_record(self, root, contents=b"actual inert license\n"):
        oid, digest = identity(contents)
        raw = base64.b64encode(contents)
        request = {"path": "LICENSE", "gitObjectSha1": oid, "url": "https://fixture.test/pinned"}
        row = {**request, **digest, "sourceGroup": "lss", "verified": True,
               "rawFile": oid + ".gitiles-data", "sourceFile": oid + ".source-data",
               "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest()}
        (root / row["rawFile"]).write_bytes(raw)
        (root / row["sourceFile"]).write_bytes(contents)
        return request, row

    def test_raw_and_decoded_original_bytes_are_independently_reverified(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-crashpad-audit-") as name:
            root = Path(name)
            request, row = self.source_record(root)
            self.assertEqual(SUBJECT["checked_record"](root, request, row, "lss"), b"actual inert license\n")
            (root / row["rawFile"]).write_bytes(b"mutated")
            with self.assertRaisesRegex(ValueError, "raw source bytes changed"):
                SUBJECT["checked_record"](root, request, row, "lss")

    def test_unsafe_data_filename_source_group_or_source_mutation_refused(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-crashpad-audit-") as name:
            root = Path(name)
            request, row = self.source_record(root)
            for changed in ({"rawFile": "../raw"}, {"sourceFile": "../source"}, {"sourceGroup": "main"}, {"verified": False}):
                with self.subTest(changed=changed), self.assertRaisesRegex(ValueError, "exact selected"):
                    SUBJECT["checked_record"](root, request, {**row, **changed}, "lss")
            (root / row["sourceFile"]).write_bytes(b"mutated")
            with self.assertRaisesRegex(ValueError, "source bytes changed"):
                SUBJECT["checked_record"](root, request, row, "lss")

    def lss_fixture(self):
        contents = b"real lss license\n"
        oid, _ = identity(contents)
        report = {"lssRootTree": {"value": {"entries": [
            {"name": "LICENSE", "mode": 0o100644, "type": "blob", "id": oid},
            {"name": "tests", "mode": 0o40000, "type": "tree", "id": "a" * 40}]}}}
        inputs = {"index": {"third_party/lss": {"mode": "160000", "gitObjectSha1": SUBJECT["SOURCES"]["REVISION"]}},
                  "files": {}, "references": []}
        return contents, report, inputs

    def test_root_source_keeps_real_git_path_and_uncollected_subtree_identity(self):
        contents, report, inputs = self.lss_fixture()
        SUBJECT["append_sources"](inputs, report, {"third_party/lss/LICENSE": contents})
        self.assertEqual(inputs["files"]["third_party/lss/LICENSE"]["payload"], contents)
        self.assertEqual(inputs["index"]["third_party/lss/tests"]["mode"], "040000")
        self.assertEqual(inputs["index"]["third_party/lss"]["mode"], "160000")
        self.assertNotIn(PREFIX + "third_party/lss/lss/LICENSE", inputs["index"])
        self.assertNotIn("third_party/lss/tests/LICENSE", inputs["index"])

    def test_conflicting_source_index_or_blob_refused_without_alias(self):
        contents, report, inputs = self.lss_fixture()
        inputs["index"]["third_party/lss/LICENSE"] = {"mode": "100644", "gitObjectSha1": "b" * 40}
        with self.assertRaisesRegex(ValueError, "not overwrite"):
            SUBJECT["append_sources"](inputs, report, {"third_party/lss/LICENSE": contents})
        contents, report, inputs = self.lss_fixture()
        with self.assertRaises(ValueError):
            SUBJECT["append_sources"](inputs, report, {"third_party/lss/LICENSE": b"wrong source"})

    def test_foreign_escaping_or_existing_output_rejected_before_inputs(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-crashpad-audit-") as name:
            root = Path(name)
            for output in (root / "foreign.json", root / "../crashpad-coverage-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](root, output)
            path = root / "crashpad-coverage-01.json"
            path.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, path)
            self.assertEqual(path.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
