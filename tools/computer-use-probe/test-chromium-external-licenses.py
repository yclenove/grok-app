"""External license data boundaries, never network/native/coverage claims."""
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-external-licenses.py")))


class ExternalTests(unittest.TestCase):
    def test_explicit_extra_xml_is_selected_and_symlink_missing_paths_remain_gaps(self):
        rows = [{"path": path, "mode": mode} for path, mode in
            (("LICENSE", "100644"), ("api/egl.xml", "100644"), ("api/link.xml", "120000"))]
        pins = {"registry": ("third_party/registry/src", "https://example.test/public", "0" * 40)}
        selected, _, extra, missing = SUBJECT["selected_inputs"]("registry", rows, [], pins,
            extra_paths=("api/egl.xml", "api/link.xml", "api/missing.xml"))
        self.assertEqual([row["path"] for row in selected], ["LICENSE", "api/egl.xml"])
        self.assertEqual(extra, ["api/egl.xml", "api/link.xml", "api/missing.xml"])
        self.assertEqual(missing, ["api/link.xml", "api/missing.xml"])
        self.assertEqual(SUBJECT["reviewed_extra"]("freetype", ()), SUBJECT["REVIEWED_EXTRA"]["freetype"])

    def test_foreign_duplicate_or_overbudget_extra_path_refuses_before_component_or_git(self):
        for paths in (("../escape",), ("/absolute",), ("C:/file",), ("a\\b",), ("a//b",), ("a/./b",),
                ("a", "a"), "api/egl.xml", (None,), tuple("file" + str(i) for i in range(SUBJECT["MAX_SELECTED"] + 1))):
            with tempfile.TemporaryDirectory(prefix="grok-cu-extra-paths-") as name:
                root = Path(name)
                with self.subTest(paths=str(paths)[:50]), self.assertRaises(ValueError):
                    SUBJECT["collect_component"]("v8", root, [], extra_paths=paths)
                self.assertFalse((root / "v8").exists())

    def test_googletest_group_requires_actual_deps_pin_not_stale_readme_revision(self):
        pins = SUBJECT["DAWN_TEST_PINS"]
        self.assertIs(SUBJECT["GROUPS"]["dawn-tests"], pins)
        path, repository, revision = pins["googletest"]
        report = {**self.catalog(), "gitDependencies": [{"path": path, "repository": repository,
            "revision": revision, "gitlinkRevision": revision, "gitlinkMatched": True, "condition": None}]}
        original = report["gitDependencies"][0]
        for field, value in ((None, None), ("revision", "b2b9072ecbe874f5937054653ef8f2731eb0f010"),
                ("gitlinkRevision", "0" * 40), ("condition", "checkout_android"),
                ("repository", "https://example.org/googletest.git")):
            changed = {**report, "gitDependencies": [{**original, **({field: value} if field else {})}]}
            data = json.dumps(changed).encode()
            with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(data).hexdigest()):
                if field:
                    with self.assertRaises(ValueError):
                        SUBJECT["checked_catalog"](data, pins)
                else:
                    SUBJECT["checked_catalog"](data, pins)

    def test_googletest_collection_preserves_unmeasured_use_and_original_license(self):
        text = b"original attribution\nfull permission and disclaimer\n"
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-tests-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["TREE"], git=self.fake_git({"LICENSE": text}), fetch_blobs=lambda *_args: None):
                report = SUBJECT["collect_component"]("googletest", root, [], pins=SUBJECT["DAWN_TEST_PINS"])
            selected = report["selectedFiles"][0]
            self.assertEqual((root / "googletest/blobs" / (selected["gitObjectSha1"] + ".source-data")).read_bytes(), text)
            self.assertIsNone(report["declaredCheckoutCondition"])
            for key in ("conditionsEvaluated", "generatorExecuted", "recursiveDependenciesCollected", "promoted"):
                self.assertIs(report[key], False)
            self.assertEqual(report["licenseCoverage"], "not_proven")

    def test_gpu_support_group_has_its_own_pins_and_rejects_dawn_standalone_validation_pin(self):
        pins = SUBJECT["GPU_SUPPORT_PINS"]
        self.assertIs(SUBJECT["GROUPS"]["gpu-support"], pins)
        report = {**self.catalog(), "gitDependencies": [{"path": path, "repository": repository,
            "revision": revision, "gitlinkRevision": revision, "gitlinkMatched": True, "condition": None}
            for path, repository, revision in pins.values()]}
        data = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(data).hexdigest()):
            SUBJECT["checked_catalog"](data, pins)
        row = report["gitDependencies"][1]
        row["revision"] = row["gitlinkRevision"] = "4d9108fb142e330532b8f1289e868c977f1e69cf"
        data = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(data).hexdigest()), self.assertRaises(ValueError):
            SUBJECT["checked_catalog"](data, pins)
        self.assertEqual(set(pins) & set(SUBJECT["GPU_PINS"]), set())

    def test_dawn_group_requires_original_pin_and_preserves_prior_groups(self):
        pins = SUBJECT["GPU_PINS"]
        self.assertIs(SUBJECT["GROUPS"]["gpu"], pins)
        path, repository, revision = pins["dawn"]
        self.assertEqual((path, repository, revision), ("third_party/dawn",
            "https://dawn.googlesource.com/dawn.git", "088a8d00efd1c70b58c549d355401d115904b648"))
        self.assertTrue(all("dawn" not in group for name, group in SUBJECT["GROUPS"].items() if name != "gpu"))
        entry = {"path": path, "repository": repository, "revision": revision,
                 "gitlinkRevision": revision, "gitlinkMatched": True, "condition": None}
        for field, value in ((None, None), ("condition", "checkout_android"),
                             ("repository", "https://example.org/dawn.git"), ("gitlinkMatched", False)):
            report = {**self.catalog(), "gitDependencies": [{**entry, **({field: value} if field else {})}]}
            data = json.dumps(report).encode()
            with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(data).hexdigest()):
                if field:
                    with self.assertRaises(ValueError):
                        SUBJECT["checked_catalog"](data, pins)
                else:
                    SUBJECT["checked_catalog"](data, pins)
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-contract-") as name:
            with patch.dict(SUBJECT["TREE"], git=self.fake_git({"LICENSE": b"original permission"}),
                            fetch_blobs=lambda *_args: None):
                report = SUBJECT["collect_component"]("dawn", Path(name), [], pins=pins)
        self.assertIsNone(report["declaredCheckoutCondition"])
        self.assertFalse(report["conditionsEvaluated"])
        self.assertFalse(report["promoted"])

    def test_webgl_notice_pin_keeps_actual_source_identity_and_null_condition(self):
        pins = SUBJECT["NOTICE_TEST_PINS"]
        self.assertIs(SUBJECT["GROUPS"]["notice-tests"], pins)
        path, repository, revision = pins["webgl"]
        report = self.catalog()
        entry = {"path": path, "repository": repository, "revision": revision,
                 "gitlinkRevision": revision, "gitlinkMatched": True, "condition": None}
        report["gitDependencies"].append(entry)
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            SUBJECT["checked_catalog"](contents, pins)
        for field, value in (("condition", "checkout_android"), ("revision", "0" * 40)):
            changed = {**report, "gitDependencies": report["gitDependencies"][:-1] + [{**entry, field: value}]}
            data = json.dumps(changed).encode()
            with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(data).hexdigest()), self.assertRaises(ValueError):
                SUBJECT["checked_catalog"](data, pins)

    def catalog(self):
        rows = [{"path": path, "repository": url, "revision": oid,
                 "gitlinkRevision": oid, "gitlinkMatched": True, "condition": None}
                for path, url, oid in SUBJECT["PINS"].values()]
        return {"sourceCommit": SUBJECT["TREE"]["COMMIT"], "gitDependencies": rows,
                "unresolved": [], "mismatches": [], "uncataloguedGitlinks": [],
                "hooksExecuted": False, "conditionsEvaluated": False}

    def checked(self, report):
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__,
                        CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            return SUBJECT["checked_catalog"](contents)

    def test_all_twelve_public_exact_pins_must_match_parent_catalog(self):
        self.assertEqual(len(self.checked(self.catalog())["gitDependencies"]), 12)

    def test_seven_media_pins_are_distinct_and_must_match_same_reviewed_catalog(self):
        report = self.catalog()
        report["gitDependencies"].extend({"path": path, "repository": url, "revision": oid,
                                          "gitlinkRevision": oid, "gitlinkMatched": True, "condition": None}
                                         for path, url, oid in SUBJECT["MEDIA_PINS"].values())
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__,
                        CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            self.assertEqual(len(SUBJECT["checked_catalog"](contents, SUBJECT["MEDIA_PINS"])["gitDependencies"]), 19)
        self.assertEqual(set(SUBJECT["PINS"]) & set(SUBJECT["MEDIA_PINS"]), set())

    def test_arbitrary_group_or_wrong_group_key_reject_before_path_or_git(self):
        for pins, keys in ((dict(SUBJECT["PINS"]), ["v8"]), (SUBJECT["MEDIA_PINS"], ["v8"]),
                           (SUBJECT["PINS"], ["libwebp"]), (dict(SUBJECT["RUNTIME_PINS"]), ["quiche"]),
                           (SUBJECT["RUNTIME_PINS"], ["v8"]), (SUBJECT["MEDIA_PINS"], ["wayland"]),
                           (dict(SUBJECT["SUPPORT_PINS"]), ["libcxx"]), (SUBJECT["SUPPORT_PINS"], ["angle"]),
                           (dict(SUBJECT["BROWSER_PINS"]), ["ink"]), (SUBJECT["BROWSER_PINS"], ["angle"])):
            with self.subTest(keys=keys), self.assertRaises(ValueError):
                SUBJECT["collect"](Path("foreign"), Path("foreign/output"), keys, pins=pins)

    def runtime_catalog(self):
        return {**self.catalog(), "gitDependencies": [
            {"path": path, "repository": url, "revision": oid,
             "gitlinkRevision": oid, "gitlinkMatched": True,
             "condition": SUBJECT["SOURCE_CONDITIONS"].get(key)}
            for key, (path, url, oid) in SUBJECT["RUNTIME_PINS"].items()]}

    def test_runtime_seventeen_public_pins_and_exact_linux_conditions(self):
        report = self.runtime_catalog()
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            actual = SUBJECT["checked_catalog"](contents, SUBJECT["RUNTIME_PINS"])
        self.assertEqual(len(actual["gitDependencies"]), 17)
        self.assertEqual(sum(row["condition"] == "checkout_linux" for row in actual["gitDependencies"]), 5)
        self.assertEqual(set(SUBJECT["RUNTIME_PINS"]) & (set(SUBJECT["PINS"]) | set(SUBJECT["MEDIA_PINS"])), set())
        prefixes = [row[0] for row in SUBJECT["RUNTIME_PINS"].values()]
        self.assertEqual(len(set(prefixes)), 17)

    def test_conditional_source_cannot_drop_broaden_or_evaluate_the_declared_condition(self):
        for value in (None, True, "checkout_win", "checkout_linux or checkout_win", "not not checkout_linux"):
            report = self.runtime_catalog()
            next(row for row in report["gitDependencies"] if row["condition"])["condition"] = value
            contents = json.dumps(report).encode()
            with self.subTest(value=value), patch.dict(SUBJECT["checked_catalog"].__globals__,
                    CATALOG_SHA=hashlib.sha256(contents).hexdigest()), self.assertRaises(ValueError):
                SUBJECT["checked_catalog"](contents, SUBJECT["RUNTIME_PINS"])

    def test_runtime_nonstandard_parent_license_file_selected_without_source_execution(self):
        prefix = SUBJECT["RUNTIME_PINS"]["wuffs"][0]
        rows = [{"path": path, "mode": "100644"} for path in ("release/c/wuffs-v0.3.c", "LICENSE", "README.md")]
        picked, refs, _, missing = SUBJECT["selected_inputs"](
            "wuffs", rows, [{"sourcePath": prefix + "/release/c/wuffs-v0.3.c"}], SUBJECT["RUNTIME_PINS"])
        self.assertEqual([row["path"] for row in picked], ["release/c/wuffs-v0.3.c", "LICENSE"])
        self.assertEqual(len(refs), 1)
        self.assertEqual(missing, [])

    def test_support_eleven_pins_and_exact_windows_conditions_match_parent(self):
        report = {**self.catalog(), "gitDependencies": [
            {"path": path, "repository": url, "revision": oid, "gitlinkRevision": oid,
             "gitlinkMatched": True, "condition": SUBJECT["SOURCE_CONDITIONS"].get(key)}
            for key, (path, url, oid) in SUBJECT["SUPPORT_PINS"].items()]}
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            actual = SUBJECT["checked_catalog"](contents, SUBJECT["SUPPORT_PINS"])
        self.assertEqual(len(actual["gitDependencies"]), 11)
        self.assertEqual(sum(row["condition"] == "checkout_win" for row in actual["gitDependencies"]), 2)
        expected = sum(len(pins) for pins in SUBJECT["GROUPS"].values())
        self.assertEqual(len({key for pins in SUBJECT["GROUPS"].values() for key in pins}), expected)
        self.assertEqual(len({row[0] for pins in SUBJECT["GROUPS"].values() for row in pins.values()}), expected)

    def browser_catalog(self):
        return {**self.catalog(), "gitDependencies": [
            {"path": path, "repository": url, "revision": oid, "gitlinkRevision": oid,
             "gitlinkMatched": True, "condition": SUBJECT["SOURCE_CONDITIONS"].get(key)}
            for key, (path, url, oid) in SUBJECT["BROWSER_PINS"].items()]}

    def test_browser_ten_pins_and_openxr_declared_condition_match_parent(self):
        report = self.browser_catalog()
        contents = json.dumps(report).encode()
        with patch.dict(SUBJECT["checked_catalog"].__globals__, CATALOG_SHA=hashlib.sha256(contents).hexdigest()):
            actual = SUBJECT["checked_catalog"](contents, SUBJECT["BROWSER_PINS"])
        self.assertEqual(len(actual["gitDependencies"]), 10)
        self.assertEqual(sum(row["condition"] == "checkout_openxr" for row in actual["gitDependencies"]), 1)
        self.assertEqual(SUBJECT["BROWSER_PINS"]["ink"][0], "third_party/ink/src")

    def test_openxr_condition_not_dropped_evaluated_or_replaced_by_an_os_guess(self):
        for value in (None, True, "checkout_win", "checkout_linux", "checkout_openxr or checkout_win"):
            report = self.browser_catalog()
            next(row for row in report["gitDependencies"] if row["condition"])["condition"] = value
            contents = json.dumps(report).encode()
            with self.subTest(value=value), patch.dict(SUBJECT["checked_catalog"].__globals__,
                    CATALOG_SHA=hashlib.sha256(contents).hexdigest()), self.assertRaises(ValueError):
                SUBJECT["checked_catalog"](contents, SUBJECT["BROWSER_PINS"])

    def test_windows_declared_condition_cannot_drop_or_change_platform(self):
        for value in (None, True, "checkout_linux", "checkout_win or checkout_linux"):
            report = {**self.catalog(), "gitDependencies": [
                {"path": path, "repository": url, "revision": oid, "gitlinkRevision": oid,
                 "gitlinkMatched": True, "condition": value if key == "microsoft-webauthn" else SUBJECT["SOURCE_CONDITIONS"].get(key)}
                for key, (path, url, oid) in SUBJECT["SUPPORT_PINS"].items()]}
            contents = json.dumps(report).encode()
            with self.subTest(value=value), patch.dict(SUBJECT["checked_catalog"].__globals__,
                    CATALOG_SHA=hashlib.sha256(contents).hexdigest()), self.assertRaises(ValueError):
                SUBJECT["checked_catalog"](contents, SUBJECT["SUPPORT_PINS"])

    def test_parent_patent_reference_is_selected_in_media_group(self):
        prefix = SUBJECT["MEDIA_PINS"]["libwebp"][0]
        rows = [{"path": path, "mode": "100644"} for path in ("PATENTS", "COPYING", "unrelated.cc")]
        picked, refs, _, missing = SUBJECT["selected_inputs"](
            "libwebp", rows, [{"sourcePath": prefix + "/PATENTS"}], SUBJECT["MEDIA_PINS"])
        self.assertEqual([row["path"] for row in picked], ["PATENTS", "COPYING"])
        self.assertEqual(len(refs), 1)
        self.assertEqual(missing, [])

    def test_changed_catalog_bytes_reject_before_parsing(self):
        with self.assertRaisesRegex(ValueError, "SHA256"):
            SUBJECT["checked_catalog"](b"not the fixed catalog")

    def test_parent_mismatch_unresolved_extra_gitlink_hooks_and_conditions_reject(self):
        for key in ("mismatches", "unresolved", "uncataloguedGitlinks", "hooksExecuted", "conditionsEvaluated"):
            report = self.catalog()
            report[key] = True if isinstance(report[key], bool) else ["gap"]
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.checked(report)

    def test_missing_duplicate_changed_remote_revision_or_condition_reject(self):
        reports = []
        report = self.catalog()
        report["gitDependencies"].pop()
        reports.append(report)
        report = self.catalog()
        report["gitDependencies"].append(report["gitDependencies"][0])
        reports.append(report)
        for key, value in (("repository", "https://example.org/unreviewed.git"),
                           ("revision", "f" * 40), ("gitlinkRevision", "0" * 40),
                           ("gitlinkMatched", False), ("condition", "checkout_private")):
            report = self.catalog()
            report["gitDependencies"][0][key] = value
            reports.append(report)
        for report in reports:
            with self.subTest(report=report), self.assertRaises(ValueError):
                self.checked(report)

    def test_checkout_refs_do_not_confuse_chromium_root_and_dependency_root(self):
        refs = SUBJECT["checkout_references"]("third_party/foo", {"path": "README.chromium"},
                                              b"License File: LICENSE, /LICENSE, ../COPYING\n")
        self.assertEqual([row["checkoutPath"] for row in refs],
                         ["third_party/foo/LICENSE", "LICENSE", "third_party/COPYING"])
        self.assertEqual([row["repositoryPath"] for row in refs], ["LICENSE", None, None])

    def test_parent_of_nested_repo_can_refer_to_other_checkout_but_not_escape_chromium(self):
        refs = SUBJECT["checkout_references"]("third_party/foo/src", {"path": "README.chromium"},
                                              b"License File: ../LICENSE\n")
        self.assertEqual(refs[0]["checkoutPath"], "third_party/foo/LICENSE")
        self.assertIsNone(refs[0]["repositoryPath"])
        with self.assertRaises(ValueError):
            SUBJECT["checkout_references"]("v8", {"path": "README.v8"}, b"License File: ../../secret\n")

    def test_remote_ref_retained_never_becomes_path_or_request(self):
        refs = SUBJECT["checkout_references"]("v8", {"path": "README.v8"},
                                              b"License File: https://example.org/LICENSE\n")
        self.assertIsNone(refs[0]["checkoutPath"])
        self.assertIsNone(refs[0]["sourcePath"])

    def test_nonmetadata_source_and_deps_are_data_never_executed(self):
        self.assertEqual(SUBJECT["checkout_references"]("v8", {"path": "DEPS"},
                                                       b"raise RuntimeError('not executed')"), [])

    def test_nonstandard_main_reference_selected_even_without_license_filename(self):
        rows = [{"path": path, "mode": "100644"} for path in ("docs/FTL.TXT", "README.md")]
        refs = [{"sourcePath": "third_party/freetype/src/docs/FTL.TXT"},
                {"sourcePath": "third_party/other/LICENSE"}, {"sourcePath": None}]
        picked, actual_refs, extra, missing = SUBJECT["selected_inputs"]("freetype", rows, refs)
        self.assertEqual([row["path"] for row in picked], ["docs/FTL.TXT"])
        self.assertEqual(len(actual_refs), 1)
        self.assertIn("docs/GPLv2.TXT", missing)
        self.assertNotIn("docs/FTL.TXT", missing)
        self.assertIn("src/gzip/zlib.h", extra)

    def test_symlink_parent_reference_never_materialized_and_gap_remains(self):
        picked, refs, _, missing = SUBJECT["selected_inputs"](
            "v8", [{"path": "LICENSE", "mode": "120000"}], [{"sourcePath": "v8/LICENSE"}])
        self.assertEqual(picked, [])
        self.assertEqual(len(refs), 1)
        self.assertEqual(missing, ["LICENSE"])

    def test_depot_metadata_helper_source_selected_as_data_not_imported(self):
        rows = [{"path": path, "mode": "100644"} for path in ("metadata/parse.py", "other.py")]
        picked, _, _, _ = SUBJECT["selected_inputs"]("depot-tools", rows, [])
        self.assertEqual([row["path"] for row in picked], ["metadata/parse.py"])

    def test_source_cache_requires_safe_generation_before_read(self):
        for name in ("../escape", "external-license-bad", "C:/external-license-01"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["checked_cache"](Path("foreign"), name)
        self.assertIsNone(SUBJECT["checked_cache"](Path("foreign"), None))

    def test_incomplete_or_wrong_pin_cache_rejects(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            source = Path(name)
            output = source / "external-license-01"
            output.mkdir()
            SUBJECT["write_report"](output / "external-source-inventory.json",
                                    {"sourceCommit": SUBJECT["TREE"]["COMMIT"],
                                     "parentCatalogSha256": SUBJECT["CATALOG_SHA"], "collectionComplete": False})
            with self.assertRaises(ValueError):
                SUBJECT["checked_cache"](source, "external-license-01")

    def test_foreign_source_output_traversal_and_bad_generation_reject_before_git(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            root = Path(name)
            source = root / ".run" / "maintained-notice-source-20261013-01"
            source.mkdir(parents=True)
            with patch.dict(SUBJECT["checked_paths"].__globals__, HERE=root):
                for output in (root / "external-license-01", source / "../escape", source / "external-license-bad"):
                    with self.subTest(output=output), self.assertRaises(ValueError):
                        SUBJECT["checked_paths"](source, output)

    def test_existing_generation_refuses_overwrite(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            root = Path(name)
            source = root / ".run" / "maintained-notice-source-20261013-01"
            output = source / "external-license-01"
            output.mkdir(parents=True)
            with patch.dict(SUBJECT["checked_paths"].__globals__, HERE=root), self.assertRaises(FileExistsError):
                SUBJECT["checked_paths"](source, output)

    def test_duplicate_or_unknown_components_fail_before_path_or_git(self):
        for keys in ([], ["v8", "v8"], ["unknown"]):
            with self.subTest(keys=keys), self.assertRaises(ValueError):
                SUBJECT["collect"](Path("foreign"), Path("foreign/output"), keys)

    def test_report_never_overwrites_existing_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            output = Path(name) / "receipt.json"
            SUBJECT["write_report"](output, {"promoted": False})
            before = output.read_bytes()
            with self.assertRaises(FileExistsError):
                SUBJECT["write_report"](output, {"promoted": True})
            self.assertEqual(before, output.read_bytes())

    def fake_git(self, contents, corrupt=False):
        blobs = {hashlib.sha1(b"blob " + str(len(value)).encode() + b"\0" + value).hexdigest(): value
                 for value in contents.values()}
        tree = b"".join(f"100644 blob {oid}\t{path}\0".encode() for path, value in contents.items()
                        for oid in [hashlib.sha1(b"blob " + str(len(value)).encode() + b"\0" + value).hexdigest()])

        def git(_bare, args, _limit=0, input_data=None):
            if args[0] in ("init", "fetch"):
                return b""
            if args[:2] == ["rev-parse", "--is-bare-repository"]:
                return b"true\n"
            if args[0] == "rev-parse":
                return (args[1] if not args[1].endswith("^{tree}") else "a" * 40).encode() + b"\n"
            if args[0] == "ls-tree":
                return tree
            if args == ["cat-file", "--batch"]:
                result = b""
                for oid in input_data.decode().splitlines():
                    value = blobs[oid]
                    if corrupt and value == b"nonstandard license":
                        value = b"x" * len(value)
                    result += f"{oid} blob {len(value)}\n".encode() + value + b"\n"
                return result
            self.fail(f"unexpected test Git operation {args[0]}")
        return git

    def test_two_round_collection_keeps_git_ids_and_nonstandard_ref_without_checkout(self):
        contents = {"README.v8": b"License File: oddball.txt\n", "LICENSE": b"root license",
                    "oddball.txt": b"nonstandard license"}
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["TREE"], git=self.fake_git(contents), fetch_blobs=lambda *_args: None):
                report = SUBJECT["collect_component"]("v8", root, [])
            self.assertTrue(report["collectionComplete"])
            self.assertEqual(report["uniqueBlobs"], 3)
            self.assertEqual(report["missingReferences"], [])
            self.assertEqual(report["selectedFiles"][-1]["path"], "oddball.txt")
            self.assertFalse((root / "v8" / "LICENSE").exists())
            self.assertFalse(report["recursiveDependenciesCollected"])
            self.assertFalse(report["promoted"])

    def test_late_corrupt_blob_preserves_partial_receipt_not_false_success(self):
        contents = {"README.v8": b"License File: oddball.txt\n", "LICENSE": b"root license",
                    "oddball.txt": b"nonstandard license"}
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            root = Path(name)
            with patch.dict(SUBJECT["TREE"], git=self.fake_git(contents, corrupt=True),
                            fetch_blobs=lambda *_args: None), self.assertRaises(ValueError):
                SUBJECT["collect_component"]("v8", root, [])
            report = json.loads((root / "v8" / "license-source-inventory.json").read_bytes())
            self.assertFalse(report["collectionComplete"])
            self.assertEqual(report["failureType"], "ValueError")
            self.assertEqual(report["uniqueBlobs"], 2)
            self.assertEqual(len(report["selectedFiles"]), 2)
            self.assertEqual(report["licenseCoverage"], "not_proven")

    def test_runtime_collector_preserves_linux_condition_as_data_only(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            with patch.dict(SUBJECT["TREE"], git=self.fake_git({"COPYING": b"license text"}),
                            fetch_blobs=lambda *_args: None):
                report = SUBJECT["collect_component"]("wayland", Path(name), [], pins=SUBJECT["RUNTIME_PINS"])
        self.assertEqual(report["declaredCheckoutCondition"], "checkout_linux")
        self.assertFalse(report["conditionsEvaluated"])
        self.assertFalse(report["promoted"])
        self.assertFalse(report["generatorExecuted"])

    def test_support_collector_preserves_windows_condition_not_a_usage_claim(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-external-contract-") as name:
            with patch.dict(SUBJECT["TREE"], git=self.fake_git({"LICENSE": b"license text"}),
                            fetch_blobs=lambda *_args: None):
                report = SUBJECT["collect_component"]("microsoft-webauthn", Path(name), [], pins=SUBJECT["SUPPORT_PINS"])
        self.assertEqual(report["declaredCheckoutCondition"], "checkout_win")
        self.assertFalse(report["conditionsEvaluated"])
        self.assertFalse(report["promoted"])


if __name__ == "__main__":
    unittest.main()
