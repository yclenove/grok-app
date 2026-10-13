"""Source identity and closure classification, not shipping license approval."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-license-input-gaps.py")))


class GapTests(unittest.TestCase):
    def test_resolution_distinguishes_verified_gap_remote_and_symlink(self):
        index = {"LICENSE": {"mode": "100644"}, "missing-bytes": {"mode": "100755"},
                 "link": {"mode": "120000"}, "gitlink": {"mode": "160000"}}
        for path, expected in ((None, "remote_reference_not_followed"),
                               ("absent", "path_not_in_collected_checkout_trees"),
                               ("link", "reference_is_not_regular_source_file"),
                               ("gitlink", "reference_is_not_regular_source_file"),
                               ("missing-bytes", "regular_source_exists_but_bytes_not_collected"),
                               ("LICENSE", "resolved_to_verified_source_bytes")):
            with self.subTest(path=path):
                self.assertEqual(SUBJECT["classify"]({"sourcePath": path}, index, {"LICENSE": {}}), expected)

    def test_cross_repository_reference_uses_combined_checkout_identity(self):
        index = {"third_party/foo/LICENSE": {"mode": "100644"}}
        ref = {"sourcePath": "third_party/foo/LICENSE", "metadata": "third_party/bar/README.chromium"}
        self.assertEqual(SUBJECT["classify"](ref, index, {"third_party/foo/LICENSE": {}}),
                         "resolved_to_verified_source_bytes")

    def test_overlapping_source_tree_identities_reject(self):
        index = {}
        rows = [{"path": "LICENSE", "mode": "100644"}]
        SUBJECT["merge_tree"](index, rows, "v8")
        with self.assertRaises(ValueError):
            SUBJECT["merge_tree"](index, rows, "v8")

    def source(self, root, contents=b"License File: LICENSE\n", path="README.chromium"):
        oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
        row = {"path": path, "mode": "100644", "gitObjectSha1": oid,
               "bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}
        (root / "blobs").mkdir(exist_ok=True)
        (root / "blobs" / (oid + ".source-data")).write_bytes(contents)
        return row

    def test_selected_metadata_bytes_reverified_and_reference_regenerated(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-closure-contract-") as name:
            root = Path(name)
            row = self.source(root)
            files, references = {}, []
            SUBJECT["merge_files"]({"v8/README.chromium": row}, [row], root, "v8", files, references)
            self.assertEqual(references[0]["sourcePath"], "v8/LICENSE")
            self.assertEqual(files["v8/README.chromium"]["gitObjectSha1"], row["gitObjectSha1"])

    def test_policy_receives_original_verified_payload_not_second_disk_read(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-closure-contract-") as name:
            root = Path(name)
            row = self.source(root)
            files = {}
            SUBJECT["merge_files"]({"README.chromium": row}, [row], root, "", files, [])
            (root / "blobs" / (row["gitObjectSha1"] + ".source-data")).write_bytes(b"later different file")
            self.assertEqual(files["README.chromium"]["payload"], b"License File: LICENSE\n")

    def test_altered_bytes_hash_or_length_and_nonregular_file_reject(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-closure-contract-") as name:
            root = Path(name)
            row = self.source(root)
            for key, value in (("bytes", 0), ("sha256", "0" * 64), ("mode", "120000")):
                bad = {**row, key: value}
                with self.subTest(key=key), self.assertRaises(ValueError):
                    SUBJECT["merge_files"]({"README.chromium": row}, [bad], root, "", {}, [])
            blob = root / "blobs" / (row["gitObjectSha1"] + ".source-data")
            blob.write_bytes(b"modified")
            with self.assertRaises(ValueError):
                SUBJECT["merge_files"]({"README.chromium": row}, [row], root, "", {}, [])

    def test_missing_tree_identity_or_duplicate_raw_path_reject(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-closure-contract-") as name:
            root = Path(name)
            row = self.source(root)
            for index, files in (({}, {}), ({"README.chromium": row}, {"README.chromium": {}})):
                with self.subTest(index=index), self.assertRaises(ValueError):
                    SUBJECT["merge_files"](index, [row], root, "", files, [])

    def test_receipt_hash_and_byte_budget_checked_before_parse(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-closure-contract-") as name:
            output = Path(name) / "receipt.json"
            output.write_bytes(b"{}")
            with self.assertRaises(ValueError):
                SUBJECT["receipt"](output, "0" * 64)
            with self.assertRaises(ValueError):
                SUBJECT["read_data"](output, 1)

    def test_foreign_source_or_unsafe_output_fails_before_read(self):
        with self.assertRaises(ValueError):
            SUBJECT["inspect"](Path("foreign"), Path("foreign/combined-license-inputs-01.json"))

    def test_every_compiled_source_has_an_independent_complete_receipt_seal(self):
        self.assertEqual(set(SUBJECT["CORE_SEALS"]), set(SUBJECT["EXTERNAL"]["PINS"]))
        self.assertEqual(set(SUBJECT["MEDIA_SEALS"]), set(SUBJECT["EXTERNAL"]["MEDIA_PINS"]))
        self.assertEqual(set(SUBJECT["RUNTIME_SEALS"]), set(SUBJECT["EXTERNAL"]["RUNTIME_PINS"]))
        self.assertEqual(set(SUBJECT["SUPPORT_SEALS"]), set(SUBJECT["EXTERNAL"]["SUPPORT_PINS"]))
        self.assertEqual(set(SUBJECT["BROWSER_SEALS"]), set(SUBJECT["EXTERNAL"]["BROWSER_PINS"]))
        self.assertEqual(set(SUBJECT["GPU_SEALS"]), set(SUBJECT["EXTERNAL"]["GPU_PINS"]))
        self.assertEqual(set(SUBJECT["GPU_SUPPORT_SEALS"]), set(SUBJECT["EXTERNAL"]["GPU_SUPPORT_PINS"]))
        for seal in (*SUBJECT["CORE_SEALS"].values(), *SUBJECT["MEDIA_SEALS"].values(),
                     *SUBJECT["RUNTIME_SEALS"].values(), *SUBJECT["SUPPORT_SEALS"].values(),
                     *SUBJECT["BROWSER_SEALS"].values(), *SUBJECT["GPU_SEALS"].values(), *SUBJECT["GPU_SUPPORT_SEALS"].values()):
            self.assertRegex(seal, r"^[0-9a-f]{64}$")

    def test_arbitrary_generation_and_unsealed_source_reject_before_read(self):
        for name, sha in (("../escape", "a" * 64), ("external-license-02", "unsealed")):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["append_external"](Path("foreign"), name, sha, {}, {}, {}, {}, [], [])
        with self.assertRaises(ValueError):
            SUBJECT["load_inputs"](Path("foreign"), include_media="yes")
        for value in (None, "yes", 1, []):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT["load_inputs"](Path("foreign"), include_runtime=value)
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT["load_inputs"](Path("foreign"), include_support=value)
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "explicit boolean"):
                SUBJECT["load_inputs"](Path("foreign"), include_browser=value)

    def test_generation_cannot_alias_another_group_or_accept_copied_pins_and_seals(self):
        pins, seals = SUBJECT["EXTERNAL"]["BROWSER_PINS"], SUBJECT["BROWSER_SEALS"]
        for name, sha, candidate_pins, candidate_seals in (
                ("external-license-05", SUBJECT["SUPPORT_SHA"], pins, seals),
                ("external-license-06", "a" * 64, pins, seals),
                ("external-license-06", SUBJECT["BROWSER_SHA"], dict(pins), seals),
                ("external-license-06", SUBJECT["BROWSER_SHA"], pins, dict(seals))):
            with self.subTest(name=name, sha=sha), \
                    patch.dict(SUBJECT["append_external"].__globals__,
                               receipt=lambda *_: self.fail("foreign group must reject before receipt read")), \
                    self.assertRaisesRegex(ValueError, "fixed reviewed external"):
                SUBJECT["append_external"](Path("foreign"), name, sha, candidate_pins,
                                            candidate_seals, {}, {}, [], [])

    def test_runtime_condition_mismatch_or_evaluation_rejects_before_tree_read(self):
        pins = SUBJECT["EXTERNAL"]["RUNTIME_PINS"]
        root = {"sourceCommit": SUBJECT["TREE"]["COMMIT"], "collectionComplete": True,
                "parentCatalogSha256": SUBJECT["EXTERNAL"]["CATALOG_SHA"], "requestedComponents": list(pins)}
        key = next(iter(pins))
        prefix, repository, revision = pins[key]
        for condition, evaluated in (("checkout_linux", False), (None, True), (None, 0)):
            component = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                         "collectionComplete": True, "declaredCheckoutCondition": condition,
                         "conditionsEvaluated": evaluated}
            with self.subTest(condition=condition, evaluated=evaluated), \
                    patch.dict(SUBJECT["append_external"].__globals__,
                               receipt=lambda path, _seal: (root if path.name == "external-source-inventory.json"
                                                            else component, "a" * 64)), \
                    self.assertRaisesRegex(ValueError, "checkout condition"):
                SUBJECT["append_external"](Path("foreign"), "external-license-04", SUBJECT["RUNTIME_SHA"],
                                            pins, SUBJECT["RUNTIME_SEALS"], {}, {}, [], [])

    def test_support_windows_condition_and_evaluated_flag_reject_before_tree_read(self):
        pins = SUBJECT["EXTERNAL"]["SUPPORT_PINS"]
        key = "microsoft-webauthn"
        root = {"sourceCommit": SUBJECT["TREE"]["COMMIT"], "collectionComplete": True,
                "parentCatalogSha256": SUBJECT["EXTERNAL"]["CATALOG_SHA"],
                "requestedComponents": [key, *(name for name in pins if name != key)]}
        prefix, repository, revision = pins[key]
        for condition, evaluated in ((None, False), ("checkout_linux", False), ("checkout_win", True)):
            component = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                         "collectionComplete": True, "declaredCheckoutCondition": condition,
                         "conditionsEvaluated": evaluated}
            with self.subTest(condition=condition, evaluated=evaluated), \
                    patch.dict(SUBJECT["append_external"].__globals__,
                               receipt=lambda path, _seal: (root if path.name == "external-source-inventory.json"
                                                            else component, "a" * 64)), \
                    self.assertRaisesRegex(ValueError, "checkout condition"):
                SUBJECT["append_external"](Path("foreign"), "external-license-05", SUBJECT["SUPPORT_SHA"],
                                            pins, SUBJECT["SUPPORT_SEALS"], {}, {}, [], [])

    def test_browser_openxr_condition_cannot_be_lost_broadened_or_evaluated(self):
        pins = SUBJECT["EXTERNAL"]["BROWSER_PINS"]
        key = "openxr"
        root = {"sourceCommit": SUBJECT["TREE"]["COMMIT"], "collectionComplete": True,
                "parentCatalogSha256": SUBJECT["EXTERNAL"]["CATALOG_SHA"],
                "requestedComponents": [key, *(name for name in pins if name != key)]}
        prefix, repository, revision = pins[key]
        for condition, evaluated in ((None, False), ("checkout_linux", False),
                                     ("checkout_openxr or checkout_win", False),
                                     ("checkout_openxr", True), ("checkout_openxr", 0)):
            component = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                         "collectionComplete": True, "declaredCheckoutCondition": condition,
                         "conditionsEvaluated": evaluated}
            with self.subTest(condition=condition, evaluated=evaluated), \
                    patch.dict(SUBJECT["append_external"].__globals__,
                               receipt=lambda path, _seal: (root if path.name == "external-source-inventory.json"
                                                            else component, "a" * 64)), \
                    self.assertRaisesRegex(ValueError, "checkout condition"):
                SUBJECT["append_external"](Path("foreign"), "external-license-06", SUBJECT["BROWSER_SHA"],
                                            pins, SUBJECT["BROWSER_SEALS"], {}, {}, [], [])


if __name__ == "__main__":
    unittest.main()
