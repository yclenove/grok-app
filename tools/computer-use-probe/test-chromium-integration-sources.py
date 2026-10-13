"""Exact source roots, four actual manifests, bounded transfer and retained failure."""
import base64
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("collect-chromium-integration-sources.py")))


def oid(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


class IntegrationSourceTests(unittest.TestCase):
    def test_generated_output_samples_have_fixed_regular_paths_in_original_angle_tree(self):
        rows = SUBJECT["requests"]("angle-generated")
        self.assertEqual(len(rows), 7)
        self.assertTrue(all(row["path"].startswith("src/") and "_autogen." in row["path"] for row in rows))
        self.assertEqual(SUBJECT["source_roots"]("angle-generated"), {"angle": SUBJECT["ROOTS"]["angle"]})
        self.assertEqual(SUBJECT["git_scope"](rows), SUBJECT["git_scope"](SUBJECT["requests"]("angle-registry")))
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-generated-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "angle-generation-git-source-01", "angle-generated", True)
            output = root / "angle-generated-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "angle-generated", True)

    def test_generation_records_and_generators_keep_same_angle_revision_and_cache(self):
        rows = SUBJECT["requests"]("angle-generation")
        self.assertEqual(len(rows), 9)
        self.assertEqual(sum(row["path"].endswith(".json") for row in rows), 5)
        self.assertEqual(sum(row["path"].endswith(".py") for row in rows), 4)
        self.assertEqual(SUBJECT["source_roots"]("angle-generation"), {"angle": SUBJECT["ROOTS"]["angle"]})
        self.assertEqual(SUBJECT["git_scope"](rows), SUBJECT["git_scope"](SUBJECT["requests"]("angle-registry")))
        self.assertTrue(all(row["gitObjectSha1"] == SUBJECT["ANGLE_GENERATION_PINS"]["angle"][row["path"]] for row in rows))

    def test_generation_evidence_cannot_overwrite_registry_or_other_source_profile(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-generation-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "angle-registry-git-source-01", "angle-generation", True)
            output = root / "angle-generation-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "angle-generation", True)

    def test_registry_sources_use_fixed_angle_root_tree_and_original_cache(self):
        rows = SUBJECT["requests"]("angle-registry")
        self.assertEqual(len(rows), 4)
        self.assertEqual({row["sourceGroup"] for row in rows}, {"angle"})
        self.assertEqual({row["path"] for row in rows}, {"scripts/registry_xml.py", "scripts/run_code_generation.py",
            "scripts/egl_angle_ext.xml", "scripts/gl_angle_ext.xml"})
        self.assertEqual(SUBJECT["source_roots"]("angle-registry"), {"angle": SUBJECT["ROOTS"]["angle"]})
        self.assertEqual(SUBJECT["git_scope"](rows), (SUBJECT["ANGLE_CACHE"], SUBJECT["ROOTS"]["angle"], SUBJECT["ANGLE_CACHE_TREE"]))

    def test_registry_output_cannot_overwrite_or_borrow_other_profile_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-registry-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "angle-header-git-source-01", "angle-registry", True)
            output = root / "angle-registry-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "angle-registry", True)

    def test_data_only_executable_source_is_regular_but_link_or_changed_object_refuses(self):
        selected = SUBJECT["requests"]("angle-registry")
        prefix, repository, revision = SUBJECT["ROOTS"]["angle"]
        index = {row["sourcePath"]: {"mode": "100644", "kind": "blob", "gitObjectSha1": row["gitObjectSha1"]} for row in selected}
        path = "third_party/angle/scripts/run_code_generation.py"
        checkout = {"targets": [{"target": target, "manifest": {"src/" + prefix: {"repository": repository, "revision": revision}}}
            for target in ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")], "checkoutReceiptSha256": "1" * 64}
        original = dict(index[path])
        for mode, changed, accepted in (("100644", False, True), ("100755", False, True),
                ("120000", False, False), ("160000", False, False), ("100755", True, False)):
            index[path] = {**original, "mode": mode, **({"gitObjectSha1": "0" * 40} if changed else {})}
            with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_args: {"index": index}), patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda _source: checkout):
                if accepted:
                    _, _, comparisons = SUBJECT["checked_inputs"](Path("unused"), "angle-registry")
                    self.assertEqual(len(comparisons), 4)
                else:
                    with self.assertRaises(ValueError):
                        SUBJECT["checked_inputs"](Path("unused"), "angle-registry")

    def test_dawn_partition_sources_use_original_chromium_root_and_exact_tree_objects(self):
        rows = SUBJECT["requests"]("dawn-support")
        self.assertEqual(len(rows), 2)
        self.assertEqual(SUBJECT["source_roots"]("dawn-support"), {"chromium": SUBJECT["ROOTS"]["chromium"]})
        self.assertEqual({row["sourcePath"]: row["gitObjectSha1"] for row in rows}, {
            "base/allocator/partition_allocator/BUILD.gn": "22c177deb9d06542920e672053ec7007af3f0c69",
            "base/allocator/partition_allocator/src/partition_alloc/pointers/raw_ptr.h": "ca958b1d1190e4ae730d862fca25d135c6ca070f"})
        self.assertTrue(all(row["sourceGroup"] == "chromium" and SUBJECT["COMMIT"] in row["url"] for row in rows))

    def test_dawn_support_output_cannot_borrow_prior_or_dependency_git_evidence(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-support-") as name:
            root = Path(name)
            for output in ("dawn-embedding-source-01", "dawn-support-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "dawn-support", True)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "dawn-embedding-source-01", "dawn-support")
            output = root / "dawn-support-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "dawn-support")

    def test_dawn_complete_source_profile_uses_exact_gpu_root_and_new_cache(self):
        rows = SUBJECT["requests"]("dawn-gn")
        self.assertEqual(len(rows), 206)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 206)
        self.assertEqual({row["sourceGroup"] for row in rows}, {"dawn"})
        self.assertTrue(all(row["path"].endswith((".gn", ".gni")) for row in rows))
        self.assertEqual(SUBJECT["source_roots"]("dawn-gn"), {"dawn": SUBJECT["EXTERNAL"]["GPU_PINS"]["dawn"]})
        self.assertEqual(SUBJECT["git_scope"](rows), (SUBJECT["DAWN_CACHE"], SUBJECT["DAWN_ROOT"], SUBJECT["DAWN_CACHE_TREE"]))
        self.assertNotEqual(SUBJECT["DAWN_CACHE"], SUBJECT["SKIA_CACHE"])
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-gn-") as name:
            root = Path(name)
            for output in ("skia-gn-git-source-01", "dawn-gn-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "dawn-gn", True)
            output = root / "dawn-gn-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "dawn-gn", True)

    def test_dawn_embedding_root_and_transport_cannot_borrow_dependency_git_scope(self):
        rows = SUBJECT["requests"]("dawn-embedding")
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["sourcePath"], "build_overrides/dawn.gni")
        self.assertEqual(SUBJECT["source_roots"]("dawn-embedding"), {"chromium": SUBJECT["ROOTS"]["chromium"]})
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-embedding-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "dawn-embedding-source-01", "dawn-embedding", True)
            output = root / "dawn-embedding-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "dawn-embedding")

    def test_complete_skia_profile_has_its_own_cache_and_exact_regular_gn_paths(self):
        rows = SUBJECT["requests"]("skia-gn")
        self.assertEqual(len(rows), 109)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 109)
        self.assertTrue(all(row["sourceGroup"] == "skia" and row["path"].endswith((".gn", ".gni")) for row in rows))
        self.assertEqual(SUBJECT["source_roots"]("skia-gn"), {"skia": SUBJECT["SKIA_ROOT"]})
        self.assertEqual(SUBJECT["git_scope"](rows), (SUBJECT["SKIA_CACHE"], SUBJECT["SKIA_ROOT"], SUBJECT["SKIA_CACHE_TREE"]))
        self.assertEqual(SUBJECT["git_scope"](SUBJECT["requests"]("swiftshader-copied"))[0], SUBJECT["SWIFTSHADER_CACHE"])
        with tempfile.TemporaryDirectory(prefix="grok-cu-skia-gn-") as name:
            root = Path(name)
            for output in ("angle-gn-git-source-01", "skia-gn-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "skia-gn", True)
            output = root / "skia-gn-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "skia-gn", True)

    def test_chromium_skia_embedding_profile_preserves_actual_primary_source_root(self):
        rows = SUBJECT["requests"]("skia-embedding")
        self.assertEqual(len(rows), 4)
        self.assertEqual({row["sourceGroup"] for row in rows}, {"chromium"})
        self.assertEqual(SUBJECT["source_roots"]("skia-embedding"), {"chromium": SUBJECT["ROOTS"]["chromium"]})
        self.assertEqual({row["sourcePath"] for row in rows}, {row["path"] for row in rows})
        self.assertTrue(any(row["path"] == "skia/BUILD.gn" for row in rows))
        self.assertTrue(any(row["path"] == "skia/tools/fuzzers/BUILD.gn" for row in rows))
        with tempfile.TemporaryDirectory(prefix="grok-cu-skia-embedding-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "skia-embedding-source-01", "skia-embedding", True)
            output = root / "skia-embedding-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "skia-embedding")

    def test_copied_profile_preserves_shared_blob_and_exact_gitlink_config_paths(self):
        rows = SUBJECT["requests"]("swiftshader-copied")
        self.assertEqual(len(rows), 13)
        self.assertEqual(len({row["gitObjectSha1"] for row in rows}), 12)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 13)
        self.assertEqual(sum(row["path"].endswith(("MD5.cpp", "MD5.h")) for row in rows), 5)
        self.assertTrue(all(row["sourceGroup"] == "swiftshader" for row in rows))
        self.assertTrue(any(row["path"] == ".gitmodules" for row in rows))
        self.assertFalse(any(row["path"] == "third_party/libbacktrace/src" for row in rows))
        self.assertEqual(SUBJECT["git_scope"](rows)[0], SUBJECT["SWIFTSHADER_CACHE"])
        self.assertEqual(SUBJECT["source_roots"]("swiftshader-copied"), {"swiftshader": SUBJECT["SWIFTSHADER_ROOT"]})

    def test_copied_profile_cannot_reuse_prior_successful_profile_output(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-swiftshader-copied-") as name:
            root = Path(name)
            for output in ("swiftshader-consumer-git-source-01", "swiftshader-gn-git-source-01", "swiftshader-copied-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "swiftshader-copied", True)
            output = root / "swiftshader-copied-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "swiftshader-copied", True)

    def test_swiftshader_profile_has_its_own_source_root_and_cache_identity(self):
        rows = SUBJECT["requests"]("swiftshader-gn")
        self.assertEqual(len(rows), 28)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 28)
        self.assertTrue(all(row["sourceGroup"] == "swiftshader" and row["path"].endswith((".gn", ".gni")) for row in rows))
        self.assertEqual(SUBJECT["source_roots"]("swiftshader-gn"), {"swiftshader": SUBJECT["SWIFTSHADER_ROOT"]})
        self.assertEqual(SUBJECT["source_roots"]("angle-gn"), SUBJECT["ROOTS"])
        self.assertEqual(SUBJECT["git_scope"](rows), (SUBJECT["SWIFTSHADER_CACHE"], SUBJECT["SWIFTSHADER_ROOT"], SUBJECT["SWIFTSHADER_CACHE_TREE"]))
        self.assertEqual(SUBJECT["git_scope"](SUBJECT["requests"]("angle-headers"))[0], SUBJECT["ANGLE_CACHE"])
        self.assertTrue(any(row["path"] == "third_party/llvm-16.0/BUILD.gn" for row in rows))
        with tempfile.TemporaryDirectory(prefix="grok-cu-swiftshader-gn-") as name:
            root = Path(name)
            for output in ("angle-gn-git-source-01", "swiftshader-gn-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "swiftshader-gn", True)
            output = root / "swiftshader-gn-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "swiftshader-gn", True)

    def test_complete_header_profile_and_output_are_separate_from_gn_sources(self):
        rows = SUBJECT["requests"]("angle-headers")
        self.assertEqual(len(rows), 38)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 38)
        self.assertTrue(all(row["sourceGroup"] == "angle" and row["path"].endswith(".h") for row in rows))
        with tempfile.TemporaryDirectory(prefix="grok-cu-api-headers-") as name:
            root = Path(name)
            for output in ("angle-gn-git-source-01", "angle-header-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / output, "angle-headers", True)
            output = root / "angle-header-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "angle-headers", True)

    def test_git_object_transport_requires_explicit_complete_gn_profile_and_own_output(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-git-") as name:
            root = Path(name)
            for profile, transport in (("integration", True), ("angle-gn", None), ("cpu-tests", True)):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, root / "angle-gn-git-source-01", profile, transport)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "angle-gn-source-01", "angle-gn", True)
            output = root / "angle-gn-git-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output, "angle-gn", True)

    def test_original_git_blob_is_not_base64_and_keeps_digest_utf8_and_size_boundaries(self):
        data = b"# original GN source\n"
        contents, identity = SUBJECT["decode_blob"](data, oid(data))
        self.assertEqual(contents, data)
        self.assertEqual(identity, {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
        for changed in (data + b" ", base64.b64encode(data), b""):
            with self.assertRaises(ValueError):
                SUBJECT["decode_blob"](changed, oid(data))
        with self.assertRaises(UnicodeError):
            SUBJECT["decode_blob"](b"\xff", oid(b"\xff"))
        with patch.dict(SUBJECT["decode_blob"].__globals__, MAX_SOURCE=len(data) - 1), self.assertRaises(ValueError):
            SUBJECT["decode_blob"](data, oid(data))

    def test_git_batch_original_headers_payload_order_and_path_bindings_are_reverified(self):
        data = b"original GN\n"
        object_id = oid(data)
        rows = [{"sourcePath": "third_party/angle/a.gni", "gitObjectSha1": object_id}]
        raw = (object_id + " blob " + str(len(data)) + "\n").encode() + data + b"\n"
        with tempfile.TemporaryDirectory(prefix="grok-cu-git-batch-") as name:
            root = Path(name)
            path = root / "git-object-batch-00.raw-data"
            path.write_bytes(raw)
            batch = {"objects": [object_id], "rawFile": path.name, "rawBytes": len(raw),
                     "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": True}
            report = {"gitCacheSource": SUBJECT["ANGLE_CACHE"], "gitCacheRevision": SUBJECT["ROOTS"]["angle"][2],
                      "gitCacheRootTreeSha1": SUBJECT["ANGLE_CACHE_TREE"], "gitilesRequestsPerformed": False, "gitObjectBatches": [batch]}
            blobs = {rows[0]["sourcePath"]: data}
            self.assertTrue(SUBJECT["checked_git_batches"](root, report, rows, blobs))
            for changed in ({**report, "gitCacheRevision": "0" * 40}, {**report, "gitilesRequestsPerformed": True},
                            {**report, "gitObjectBatches": []}, {**report, "gitObjectBatches": [{**batch, "objects": []}]}):
                with self.assertRaises(ValueError):
                    SUBJECT["checked_git_batches"](root, changed, rows, blobs)
            with self.assertRaisesRegex(ValueError, "records differ"):
                SUBJECT["checked_git_batches"](root, report, rows, {rows[0]["sourcePath"]: data + b" "})
            path.write_bytes(raw + b" ")
            with self.assertRaisesRegex(ValueError, "output changed"):
                SUBJECT["checked_git_batches"](root, report, rows, blobs)

    def test_arbitrary_git_objects_are_rejected_before_cache_or_network(self):
        with self.assertRaisesRegex(ValueError, "compiled complete"):
            list(SUBJECT["fetched_git_requests"](Path("foreign"), Path("foreign/output"), [], {}))

    def test_shared_git_object_reuses_identical_data_and_never_overwrites_different_bytes(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-shared-git-blob-") as name:
            path = Path(name) / "original.source-data"
            SUBJECT["save_blob"](path, b"original", 64)
            SUBJECT["save_blob"](path, b"original", 64)
            with self.assertRaisesRegex(ValueError, "identical original"):
                SUBJECT["save_blob"](path, b"other", 64)
            with self.assertRaises(ValueError):
                SUBJECT["save_blob"](path, b"original", 1)
            self.assertEqual(path.read_bytes(), b"original")

    def test_all_angle_gn_pins_keep_regular_exact_paths_and_full_profile_registration(self):
        rows = SUBJECT["requests"]("angle-gn")
        self.assertEqual(len(rows), 162)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 162)
        self.assertTrue(all(row["sourceGroup"] == "angle" and row["path"].endswith((".gn", ".gni")) for row in rows))
        self.assertTrue(any(row["path"] == ".gn" for row in rows))
        self.assertTrue(any(row["path"] == "third_party/cpu_features/BUILD.gn" for row in rows))
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-gn-") as name:
            source = Path(name)
            output = source / "angle-gn-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](source, output, "angle-gn")

    def test_parallel_read_waves_keep_original_order_and_do_not_start_later_wave_after_failure(self):
        rows = [{"key": number} for number in range(9)]
        with patch.dict(SUBJECT["fetched_requests"].__globals__, fetch=lambda request: str(request["key"]).encode()):
            for parallel in (False, True):
                self.assertEqual(list(SUBJECT["fetched_requests"](rows, parallel)), [(row, str(row["key"]).encode()) for row in rows])
        observed = []
        def failed(request):
            observed.append(request["key"])
            if request["key"] == 0:
                raise ValueError("wave failed")
            return b"ok"
        with patch.dict(SUBJECT["fetched_requests"].__globals__, fetch=failed), self.assertRaisesRegex(ValueError, "wave failed"):
            list(SUBJECT["fetched_requests"](rows, True))
        self.assertEqual(sorted(observed), [0, 1, 2, 3])

    def test_cpu_test_profile_checks_original_gn_path_and_output_registration(self):
        rows = SUBJECT["requests"]("cpu-tests")
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["sourcePath"], "third_party/angle/src/tests/BUILD.gn")
        self.assertEqual(rows[0]["gitObjectSha1"], SUBJECT["CPU_TEST_PINS"]["angle"]["src/tests/BUILD.gn"])
        with tempfile.TemporaryDirectory(prefix="grok-cu-cpu-test-source-") as name:
            source = Path(name)
            output = source / "cpu-test-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](source, output, "cpu-tests")
            with self.assertRaises(ValueError):
                SUBJECT["collect"](source, source / "cpu-consumer-source-01", "cpu-tests")

    def test_cpu_consumer_profile_uses_real_angle_source_paths(self):
        rows = SUBJECT["requests"]("cpu-consumer")
        self.assertEqual(len(rows), 2)
        self.assertEqual({row["path"] for row in rows}, {"src/libGLESv2.gni", "src/gpu_info_util/SystemInfo_android.cpp"})
        self.assertTrue(all(row["sourceGroup"] == "angle" and row["sourcePath"] == "third_party/angle/" + row["path"] for row in rows))
        self.assertTrue(all(row["gitObjectSha1"] == SUBJECT["CPU_CONSUMER_PINS"]["angle"][row["path"]] for row in rows))
        with tempfile.TemporaryDirectory(prefix="grok-cu-cpu-source-") as name:
            source = Path(name)
            output = source / "cpu-consumer-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](source, output, "cpu-consumer")
            with self.assertRaises(ValueError):
                SUBJECT["collect"](source, source / "foreign", "cpu-consumer")

    def test_fifteen_exact_requests_use_each_repository_revision_and_real_source_path(self):
        rows = SUBJECT["requests"]()
        self.assertEqual(len(rows), 15)
        self.assertEqual(len({row["sourcePath"] for row in rows}), 15)
        for row in rows:
            prefix, repository, revision = SUBJECT["ROOTS"][row["sourceGroup"]]
            url_root = repository[:-4] if repository.endswith(".git") else repository
            self.assertEqual(row["url"], url_root + "/+/" + revision + "/" + row["path"] + "?format=TEXT")
            self.assertEqual(row["sourcePath"], prefix + "/" + row["path"] if prefix else row["path"])
            self.assertEqual(row["gitObjectSha1"], SUBJECT["PINS"][row["sourceGroup"]][row["path"]])

    def test_arbitrary_url_path_group_or_blob_refused_before_network(self):
        row = SUBJECT["requests"]()[0]
        for key, value in (("url", "https://example.test/file"), ("sourcePath", "../other"),
                           ("sourceGroup", "other"), ("gitObjectSha1", "a" * 40)):
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "compiled fixed-revision"):
                SUBJECT["fetch"]({**row, key: value})

    def test_embedding_profile_is_three_additional_fixed_source_paths(self):
        rows = SUBJECT["requests"]("embedding")
        self.assertEqual(len(rows), 3)
        self.assertEqual({row["sourcePath"] for row in rows}, {"build_overrides/build.gni",
                         "third_party/angle/build_overrides/build.gni", "third_party/angle/src/libANGLE/renderer/vulkan/BUILD.gn"})
        for row in rows:
            self.assertEqual(row["gitObjectSha1"], SUBJECT["EMBEDDING_PINS"][row["sourceGroup"]][row["path"]])

    def test_unknown_profile_and_cross_profile_output_refused_before_inputs(self):
        with self.assertRaises(ValueError):
            SUBJECT["requests"]("arbitrary")
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "integration-source-01", "embedding")
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "embedding-source-01", "integration")

    def test_targets_profile_follows_exact_url_license_header_and_vulkan_rule(self):
        rows = SUBJECT["requests"]("targets")
        self.assertEqual(len(rows), 2)
        self.assertEqual({row["sourcePath"] for row in rows}, {"url/gurl.h", "third_party/angle/src/common/vulkan/BUILD.gn"})
        for row in rows:
            self.assertEqual(row["gitObjectSha1"], SUBJECT["TARGET_PINS"][row["sourceGroup"]][row["path"]])

    def test_consumers_profile_has_exact_build_rules_and_real_copied_khronos_headers(self):
        rows = SUBJECT["requests"]("consumers")
        self.assertEqual(len(rows), 10)
        self.assertEqual(sum(row["sourceGroup"] == "chromium" for row in rows), 4)
        self.assertEqual(sum(row["sourceGroup"] == "angle" for row in rows), 6)
        for row in rows:
            self.assertEqual(row["gitObjectSha1"], SUBJECT["CONSUMER_PINS"][row["sourceGroup"]][row["path"]])
            self.assertNotIn("EGL-Registry/", row["sourcePath"])
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-") as name:
            root = Path(name)
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "integration-source-01", "consumers")
            with self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "consumer-source-01", "integration")

    def test_platform_profile_collects_real_headers_without_generating_registry_aliases(self):
        rows = SUBJECT["requests"]("platform")
        self.assertEqual(len(rows), 3)
        self.assertEqual({row["sourcePath"] for row in rows}, {"third_party/angle/include/EGL/eglplatform.h",
                         "third_party/angle/include/GLES2/gl2platform.h", "third_party/angle/include/GLES3/gl3platform.h"})
        for row in rows:
            self.assertEqual(row["gitObjectSha1"], SUBJECT["PLATFORM_PINS"]["angle"][row["path"]])

    def input_fixture(self):
        index = {row["sourcePath"]: {"mode": "100644", "kind": "blob", "gitObjectSha1": row["gitObjectSha1"]}
                 for row in SUBJECT["requests"]()}
        targets = [{"target": name, "manifest": {"name": name}} for name in ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")]
        return {"index": index, "mainSha256": "main"}, {"targets": targets, "checkoutReceiptSha256": "checkout"}

    def test_all_three_sources_match_all_four_actual_manifests(self):
        inputs, checkout = self.input_fixture()
        calls = []
        def compare(path, repository, revision, manifest):
            calls.append((path, repository, revision, manifest["name"]))
            return {"status": "exact_repository_and_revision_match"}
        with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: inputs), \
                patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda *_: checkout, compare_pin=compare):
            actual_inputs, sha, rows = SUBJECT["checked_inputs"](Path("fixture"))
            self.assertIs(actual_inputs, inputs)
            self.assertEqual(sha, "checkout")
            self.assertEqual(len(rows), 12)
            self.assertEqual(len(calls), 12)
            self.assertEqual({call[0] for call in calls}, {"src", "src/third_party/angle", "src/third_party/openscreen/src"})

    def test_missing_duplicate_manifest_or_dependency_identity_mismatch_reject(self):
        inputs, checkout = self.input_fixture()
        with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: inputs):
            for targets in (checkout["targets"][:-1], [checkout["targets"][0]] * 4):
                with patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda *_: {**checkout, "targets": targets}), self.assertRaisesRegex(ValueError, "four unique"):
                    SUBJECT["checked_inputs"](Path("fixture"))
            with patch.dict(SUBJECT["CHECKOUTS"], inspect=lambda *_: checkout,
                            compare_pin=lambda *_: {"status": "exact_manifest_entry_absent"}), self.assertRaisesRegex(ValueError, "all selected exact"):
                SUBJECT["checked_inputs"](Path("fixture"))

    def test_independent_tree_mismatch_or_symlink_refuses_before_checkout_and_fetch(self):
        for changed in ({"mode": "120000"}, {"gitObjectSha1": "a" * 40}, {"kind": "commit"}):
            inputs, _ = self.input_fixture()
            path = SUBJECT["requests"]()[0]["sourcePath"]
            inputs["index"][path].update(changed)
            with patch.dict(SUBJECT["INPUTS"], load_inputs=lambda *_: inputs), self.assertRaisesRegex(ValueError, "independent sealed"):
                SUBJECT["checked_inputs"](Path("fixture"))

    def test_valid_source_is_inert_and_matches_git_blob_and_sha256(self):
        contents = b"raise RuntimeError('never execute collected data')\n"
        data, digest = SUBJECT["decode"](base64.b64encode(contents), oid(contents))
        self.assertEqual(data, contents)
        self.assertEqual(digest, {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()})
        with self.assertRaises(ValueError):
            SUBJECT["decode"](base64.b64encode(contents + b"mutated"), oid(contents))

    def test_empty_invalid_base64_non_utf8_and_raw_source_bounds_reject(self):
        for raw in (b"", b"invalid!", b"A" * (SUBJECT["MAX_RAW"] + 1), base64.b64encode(b"x" * (SUBJECT["MAX_SOURCE"] + 1))):
            with self.subTest(length=len(raw)), self.assertRaises(ValueError):
                SUBJECT["decode"](raw, "a" * 40)
        with self.assertRaises(UnicodeDecodeError):
            SUBJECT["decode"](base64.b64encode(b"\xff"), oid(b"\xff"))

    def test_encoded_html_redirect_or_oversize_http_response_refused(self):
        request = SUBJECT["requests"]()[0]
        class Response:
            headers = {"Content-Type": "text/plain"}
            actual_url = request["url"]
            data = b"eA=="
            def geturl(self):
                return self.actual_url
            def read(self, size):
                self.requested_size = size
                return self.data[:size]
            def __enter__(self):
                return self
            def __exit__(self, *_):
                pass
        response = Response()
        class Opener:
            def open(inner, http, timeout):
                self.assertEqual(timeout, 20)
                self.assertEqual(http.full_url, request["url"])
                self.assertEqual(http.get_header("Accept-encoding"), "identity")
                return response
        with patch.object(SUBJECT["urllib"].request, "build_opener", return_value=Opener()):
            self.assertEqual(SUBJECT["fetch"](request), b"eA==")
            self.assertEqual(response.requested_size, SUBJECT["MAX_RAW"] + 1)
            for headers in ({"Content-Type": "text/html"}, {"Content-Type": "text/plain", "Content-Encoding": "gzip"}):
                response.headers = headers
                with self.assertRaises(ValueError):
                    SUBJECT["fetch"](request)
            response.headers = {"Content-Type": "text/plain"}
            response.actual_url = "https://example.test/file"
            with self.assertRaises(ValueError):
                SUBJECT["fetch"](request)
            response.actual_url = request["url"]
            response.data = b"x" * (SUBJECT["MAX_RAW"] + 1)
            with self.assertRaises(ValueError):
                SUBJECT["fetch"](request)

    def test_foreign_escaping_or_existing_output_rejected_before_inputs(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-") as name:
            root = Path(name)
            for output in (root / "foreign", root / "../integration-source-01"):
                with self.assertRaises(ValueError):
                    SUBJECT["collect"](root, output)
            output = root / "integration-source-01"
            output.mkdir()
            with self.assertRaises(FileExistsError):
                SUBJECT["collect"](root, output)

    def test_late_decode_failure_keeps_raw_and_partial_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main"}, "checkout", []), fetch=lambda *_: b"invalid!"), \
                    self.assertRaises(ValueError):
                SUBJECT["collect"](root, root / "integration-source-01")
            report = json.loads((root / "integration-source-01/integration-source-inventory.json").read_text())
            self.assertFalse(report["collectionComplete"])
            self.assertFalse(report["records"][0]["verified"])
            self.assertEqual((root / "integration-source-01" / report["records"][0]["rawFile"]).read_bytes(), b"invalid!")
            for key in ("effectiveGnMeasured", "metadataPathsAliased", "promoted"):
                self.assertFalse(report[key])

    def test_total_source_budget_retains_partial_without_writing_overbudget_source(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-") as name:
            root = Path(name)
            payload = b"x" * SUBJECT["MAX_SOURCE"]
            digest = {"bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
            with patch.dict(SUBJECT["collect"].__globals__, checked_inputs=lambda *_: ({"mainSha256": "main"}, "checkout", []),
                            fetch=lambda *_: b"bounded raw", decode=lambda *_: (payload, digest), print=lambda *_a, **_k: None), \
                    self.assertRaisesRegex(ValueError, "total byte budget"):
                SUBJECT["collect"](root, root / "integration-source-01")
            report = json.loads((root / "integration-source-01/integration-source-inventory.json").read_text())
            self.assertEqual(len(report["records"]), 9)
            self.assertFalse(report["records"][-1]["verified"])
            self.assertNotIn("sourceFile", report["records"][-1])
            self.assertFalse(report["collectionComplete"])


if __name__ == "__main__":
    unittest.main()
