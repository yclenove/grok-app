"""Real source roots, license variants and header dependency closure remain explicit."""
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-consumer-coverage.py")))


def identity(contents):
    oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
    return oid, {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}


class ConsumerCoverageTests(unittest.TestCase):
    def cpp_fixture(self):
        texts = {
            "build/config/c++/c++.gni": 'import("//build_overrides/build.gni")\n  use_custom_libcxx = true\n  use_custom_libcxx_for_host = true\n'
                'libcxx_prefix = "//third_party/libc++/src"\nlibcxxabi_prefix = "//third_party/libc++abi/src"\n',
            "buildtools/third_party/libc++/BUILD.gn": '    "//third_party/libc++/src/src/algorithm.cpp",\n  deps = [ "//third_party/llvm-libc:llvm-libc-shared" ]\n',
            "buildtools/third_party/libc++abi/BUILD.gn": 'import("//build/config/c++/c++.gni")\n    "//third_party/libc++abi/src/src/abort_message.cpp",\n',
            "third_party/llvm-libc/BUILD.gn": '  include_dirs = [ "src" ]\n  visibility = [ "//buildtools/third_party/libc++" ]\n',
        }
        targets = [{"target": name, "gnArgsWritten": {"target_os": os_name, "target_cpu": cpu},
                    "effectiveGnMeasured": False, "remoteGeneratedFileMeasured": False}
                   for name, (os_name, cpu) in SUBJECT["CRASHPAD"]["ARGS"]["PLATFORMS"].items()]
        return {path: text.encode() for path, text in texts.items()}, targets

    def test_cpp_config_keeps_real_prefixes_and_no_standalone_rewrite_or_binary_claim(self):
        blobs, targets = self.cpp_fixture()
        original = copy.deepcopy((blobs, targets))
        report = SUBJECT["cpp_configuration"](blobs, targets)
        self.assertEqual(len(report["rules"]), 11)
        self.assertEqual(report["sourceLevelSourceRoots"], ["third_party/libc++/src", "third_party/libc++abi/src", "third_party/llvm-libc/src"])
        self.assertFalse(report["standaloneDirectoriesRewritten"])
        self.assertFalse(report["binaryUsageProven"])
        self.assertEqual((blobs, targets), original)
        blobs["build/config/c++/c++.gni"] = blobs["build/config/c++/c++.gni"].replace(b"//third_party/libc++/src", b"//other/libc++")
        with self.assertRaisesRegex(ValueError, "one exact pinned"):
            SUBJECT["cpp_configuration"](blobs, targets)

    def test_cpp_override_missing_duplicate_or_changed_measurement_claim_refused(self):
        for variable in ("use_custom_libcxx", "use_custom_libcxx_for_host", "libcxx_prefix", "libcxxabi_prefix"):
            blobs, targets = self.cpp_fixture()
            targets[0]["gnArgsWritten"][variable] = "true"
            with self.subTest(variable=variable), self.assertRaisesRegex(ValueError, "override"):
                SUBJECT["cpp_configuration"](blobs, targets)
        blobs, targets = self.cpp_fixture()
        for changed in (targets[:-1], [targets[0]] * 4):
            with self.assertRaisesRegex(ValueError, "four original"):
                SUBJECT["cpp_configuration"](blobs, changed)
        targets[0]["effectiveGnMeasured"] = True
        with self.assertRaisesRegex(ValueError, "source-only"):
            SUBJECT["cpp_configuration"](blobs, targets)

    def library_fixture(self):
        inputs, declarations = {"index": {}, "files": {}}, []
        for key, (prefix, repository, revision) in SUBJECT["LIBRARIES"].items():
            inputs["index"][prefix] = {"mode": "160000", "gitObjectSha1": revision}
            data = (key + " real license\n").encode()
            oid, digest = identity(data)
            path = prefix + "/LICENSE.TXT"
            inputs["index"][path] = {"mode": "100644", "gitObjectSha1": oid}
            inputs["files"][path] = {"gitObjectSha1": oid, "payload": data, **digest}
            declarations.append({"path": prefix, "repository": repository, "revision": revision, "condition": None,
                                 "gitlinkRevision": revision, "gitlinkMatched": True})
        targets = [{"target": name, "manifest": {}} for name in SUBJECT["CRASHPAD"]["ARGS"]["PLATFORMS"]]
        return inputs, {"gitDependencies": declarations}, {"targets": targets}

    def libraries(self, inputs, catalog, checkout, status="exact_repository_and_revision_match"):
        with patch.dict(SUBJECT["INPUTS"], read_data=lambda *_: b"catalog"), \
                patch.dict(SUBJECT["INPUTS"]["EXTERNAL"], checked_catalog=lambda *_: catalog), \
                patch.dict(SUBJECT["SOURCES"]["CHECKOUTS"], inspect=lambda *_: checkout, compare_pin=lambda *_: {"status": status}):
            return SUBJECT["libraries"](Path("fixture"), inputs)

    def test_cpp_license_pins_match_real_paths_and_all_four_manifests(self):
        inputs, catalog, checkout = self.library_fixture()
        original = copy.deepcopy((inputs, catalog, checkout))
        rows = self.libraries(inputs, catalog, checkout)
        self.assertEqual(len(rows), 3)
        self.assertTrue(all(len(row["checkoutCorrespondence"]) == 4 for row in rows))
        self.assertTrue(all(row["licensePath"] == row["checkoutPath"] + "/LICENSE.TXT" for row in rows))
        self.assertTrue(all(not row["metadataPathsAliased"] and not row["binaryUsageProven"] for row in rows))
        self.assertEqual((inputs, catalog, checkout), original)

    def test_foreign_cpp_deps_gitlink_manifest_or_duplicate_target_refused(self):
        for key, value in (("condition", "is_linux"), ("revision", "a" * 40), ("repository", "https://example.test/source")):
            inputs, catalog, checkout = self.library_fixture()
            catalog["gitDependencies"][0][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "DEPS/Gitlink"):
                self.libraries(inputs, catalog, checkout)
        inputs, catalog, checkout = self.library_fixture()
        prefix = next(iter(SUBJECT["LIBRARIES"].values()))[0]
        inputs["index"][prefix]["mode"] = "100644"
        with self.assertRaisesRegex(ValueError, "DEPS/Gitlink"):
            self.libraries(inputs, catalog, checkout)
        with self.assertRaisesRegex(ValueError, "all four actual manifests"):
            self.libraries(*self.library_fixture(), status="exact_manifest_entry_absent")
        inputs, catalog, checkout = self.library_fixture()
        checkout["targets"] = [checkout["targets"][0]] * 4
        with self.assertRaisesRegex(ValueError, "four original"):
            self.libraries(inputs, catalog, checkout)

    def test_cpp_license_missing_mutated_symlink_or_digest_mismatch_refused(self):
        for change in ("missing", "payload", "symlink", "digest"):
            inputs, catalog, checkout = self.library_fixture()
            path = next(iter(inputs["files"]))
            if change == "missing":
                del inputs["files"][path]
            elif change == "payload":
                inputs["files"][path]["payload"] = b"mutated"
            elif change == "symlink":
                inputs["index"][path]["mode"] = "120000"
            else:
                inputs["files"][path]["sha256"] = "a" * 64
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.libraries(inputs, catalog, checkout)

    def header(self, spdx, years):
        copyright_line = "** Copyright " + ("(c) " if spdx is None else "") + years + " The Khronos Group Inc.\n"
        if spdx:
            body = "** SPDX-License-Identifier: " + spdx + "\n"
        else:
            body = '** Permission is hereby granted, free of charge\n** distribute, sublicense, and/or sell copies of the Materials\n'
            body += '** The above copyright notice and this permission notice shall be included\n** THE MATERIALS ARE PROVIDED "AS IS"\n'
            body += '** NONINFRINGEMENT.\n** MATERIALS OR THE USE OR OTHER DEALINGS IN THE MATERIALS.\n'
        return ("/*\n" + copyright_line + body + "*/\n").encode()

    def test_per_file_spdx_and_copyright_remain_exact_and_brief_is_not_full_license(self):
        specification = ("Apache-2.0", "2013-2020")
        data = self.header(*specification)
        row = SUBJECT["attribution"](data, "include/EGL/egl.h", specification)
        self.assertEqual(row["declaredSpdx"], "Apache-2.0")
        self.assertEqual(row["copyrightYears"], "2013-2020")
        self.assertTrue(row["briefSpdxHeaderNeedsFullLicense"])
        self.assertFalse(row["fullLicenseTextInBlock"])
        self.assertFalse(row["originalRegistryCommitReproduced"])
        for changed in (data.replace(b"Apache-2.0", b"MIT"), data.replace(b"2013-2020", b"2013-2026"), data + data):
            with self.assertRaises(ValueError):
                SUBJECT["attribution"](changed, "include/EGL/egl.h", specification)

    def test_inline_khronos_permission_and_disclaimer_cannot_be_truncated(self):
        specification = (None, "2008-2018")
        data = self.header(*specification)
        row = SUBJECT["attribution"](data, "include/KHR/khrplatform.h", specification)
        self.assertTrue(row["fullLicenseTextInBlock"])
        self.assertFalse(row["briefSpdxHeaderNeedsFullLicense"])
        for marker in (b'"AS IS"', b"distribute, sublicense", b"The above copyright notice"):
            with self.assertRaisesRegex(ValueError, "complete original inline"):
                SUBJECT["attribution"](data.replace(marker, b"removed"), "include/KHR/khrplatform.h", specification)

    def test_actual_header_dependencies_required_and_registry_paths_not_aliased(self):
        blobs = {SUBJECT["ANGLE"] + path: self.header(*specification) for path, specification in SUBJECT["HEADERS"].items()}
        path = SUBJECT["ANGLE"] + "include/GLES2/gl2.h"
        blobs[path] += b"#include <GLES2/gl2platform.h>\n#include <KHR/khrplatform.h>\n"
        original = copy.deepcopy(blobs)
        report = SUBJECT["copied_headers"](blobs)
        self.assertEqual(len(report["headers"]), 8)
        self.assertEqual([row["declaredSpdx"] for row in report["headers"]].count("Apache-2.0"), 5)
        self.assertTrue(report["allReviewedHeaderIncludesCollected"])
        self.assertFalse(report["fullNoticeComplete"])
        self.assertFalse(report["registryMetadataPathsAliased"])
        self.assertEqual(blobs, original)
        with self.assertRaisesRegex(ValueError, "real source path"):
            SUBJECT["copied_headers"]({key: value for key, value in blobs.items() if not key.endswith("gl2platform.h")})
        blobs[path] += b"#include <GLES2/uncollected.h>\n"
        with self.assertRaisesRegex(ValueError, "real source path"):
            SUBJECT["copied_headers"](blobs)

    def test_baseline_is_recomputed_exactly_not_trusted_by_matching_counts(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-consumer-audit-") as name:
            source = Path(name)
            report = {"after": {"verifiedSelectedFiles": 10}, "metadataPathsAliased": False}
            data = (json.dumps(report, indent=2) + "\n").encode()
            (source / "integration-coverage-04.json").write_bytes(data)
            for contents in (data, data.replace(b"\n", b"\r\n")):
                (source / "integration-coverage-04.json").write_bytes(contents)
                sha = hashlib.sha256(contents).hexdigest()
                with patch.dict(SUBJECT["recheck_baseline"].__globals__, BASELINE_SHA=sha):
                    self.assertEqual(SUBJECT["recheck_baseline"](source, report), sha)
                    with self.assertRaisesRegex(ValueError, "reproduce exactly"):
                        SUBJECT["recheck_baseline"](source, {**report, "metadataPathsAliased": True})
                    (source / "integration-coverage-04.json").write_bytes(contents + b" ")
                    with self.assertRaisesRegex(ValueError, "receipt SHA256"):
                        SUBJECT["recheck_baseline"](source, report)

    def test_foreign_escaping_or_existing_output_refused_before_loading_sources(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-consumer-audit-") as name:
            source = Path(name)
            for output in (source / "foreign.json", source / "../consumer-coverage-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](source, output)
            output = source / "consumer-coverage-01.json"
            output.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](source, output)
            self.assertEqual(output.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
