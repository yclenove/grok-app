"""Embedding evidence cannot hide raw errors, mutate real paths or execute DEPS."""
import base64
import copy
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-integration-coverage.py")))
ANGLE, SCREEN = SUBJECT["ANGLE"], SUBJECT["SCREEN"]


def identity(contents):
    oid = hashlib.sha1(b"blob " + str(len(contents)).encode() + b"\0" + contents).hexdigest()
    return oid, {"bytes": len(contents), "sha256": hashlib.sha256(contents).hexdigest()}


class IntegrationCoverageTests(unittest.TestCase):
    def test_git_object_verification_is_explicit_and_never_applied_to_other_profiles(self):
        for profile, transport in (("integration", True), ("angle-gn", None), ("cpu-tests", True)):
            with self.assertRaises(ValueError):
                SUBJECT["checked_profile"](Path("foreign"), profile, {}, "", [], git_objects=transport)

    def fixture(self):
        texts = {
            "build_overrides/build.gni": 'import("//build/config/gclient_args.gni")\n',
            ANGLE + "build_overrides/build.gni": 'declare_args() {\n  build_with_chromium = false\n}\n',
            "build_overrides/angle.gni": 'angle_has_build = true\n',
            ANGLE + "gni/angle.gni": 'import("//build_overrides/angle.gni")\nif (angle_has_build) {\n  import("//build/config/gclient_args.gni")\n}\n'
                '  angle_standalone = !build_with_chromium\n'
                '  angle_has_astc_encoder = angle_has_build && angle_standalone && !is_android\n'
                '  angle_enable_cl = false\n  if (!checkout_angle_cl_deps) {\n    angle_enable_cl = false\n  }\n',
            ".gn": 'default_args = {\n  angle_vulkan_loader_dir = "//third_party/vulkan-loader/src"\n}\n',
            ANGLE + "src/common/vulkan/BUILD.gn": '        public_deps = [ "$angle_vulkan_loader_dir:libvulkan" ]\n',
            SCREEN + "BUILD.gn": 'import("//build_overrides/build.gni")\n',
            SCREEN + "build_overrides/build.gni": 'build_with_chromium = false\n',
            "url/gurl.h": '// Copyright 2013 The Chromium Authors\n// Use of this source code is governed by a BSD-style license that can be\n'
                          '// found in the LICENSE file.\n',
        }
        for folder, target, kind in (("googleurl", "//url", "source_set"), ("quiche", "//net/third_party/quiche", "source_set"),
                                     ("perfetto", "//third_party/perfetto", "group")):
            texts[SCREEN + "third_party/" + folder + "/BUILD.gn"] = 'if (build_with_chromium) {\n  ' + kind + '("' + folder + '") {\n    public_deps = [ "' + target + '" ]\n  }\n}\n'
        targets = [{"target": name, "buildId": str(number), "gnArgsWritten": {"target_os": os_name, "target_cpu": cpu},
                    "gclientConfigRecorded": {"customVars": {}}, "explicitGnArgsVerified": True, "actualGclientCustomVarsVerified": True}
                   for number, (name, (os_name, cpu)) in enumerate(SUBJECT["CRASHPAD"]["ARGS"]["PLATFORMS"].items())]
        return {path: text.encode() for path, text in texts.items()}, {"targets": targets}

    def test_source_mapping_uses_real_root_paths_without_alias_or_error_exclusion(self):
        blobs, args = self.fixture()
        original = copy.deepcopy((blobs, args))
        report = SUBJECT["configuration"](blobs, args)
        self.assertEqual(len(report["rules"]), 16)
        self.assertEqual([row["licensePath"] for row in report["sourceLevelMapping"]],
                         ["third_party/vulkan-loader/src/LICENSE.txt", "LICENSE", "net/third_party/quiche/src/LICENSE", "third_party/perfetto/LICENSE"])
        self.assertEqual(report["sourceLevelMapping"][1]["licenseHeaderSource"], "url/gurl.h")
        for key in ("metadataPathsAliased", "originalErrorsExcluded", "effectiveGnMeasured"):
            self.assertFalse(report[key])
        self.assertTrue(all(not row["remoteGeneratedFileMeasured"] for row in report["targets"]))
        self.assertEqual((blobs, args), original)

    def test_foreign_embedded_target_loader_or_license_header_refused(self):
        for path, old, new in ((SCREEN + "third_party/perfetto/BUILD.gn", b"//third_party/perfetto", b"//other/perfetto"),
                               (".gn", b"//third_party/vulkan-loader/src", b"//other/loader"),
                               ("url/gurl.h", b"LICENSE file", b"other license file"),
                               ("build_overrides/build.gni", b"gclient_args.gni", b"other_args.gni")):
            blobs, args = self.fixture()
            blobs[path] = blobs[path].replace(old, new)
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "one exact pinned"):
                SUBJECT["configuration"](blobs, args)

    def test_duplicate_rule_does_not_silently_choose_one_branch(self):
        blobs, args = self.fixture()
        blobs["build_overrides/angle.gni"] *= 2
        with self.assertRaisesRegex(ValueError, "one exact pinned"):
            SUBJECT["configuration"](blobs, args)

    def test_gclient_import_from_a_different_scope_cannot_prove_embedding_branch(self):
        blobs, args = self.fixture()
        path = ANGLE + "gni/angle.gni"
        blobs[path] = blobs[path].replace(b'  import("//build/config/gclient_args.gni")\n}',
                                         b'}\nif (other_condition) {\n  import("//build/config/gclient_args.gni")\n}')
        with self.assertRaisesRegex(ValueError, "one exact pinned"):
            SUBJECT["configuration"](blobs, args)

    def test_actual_gn_and_custom_var_overrides_need_separate_review(self):
        for variable in ("build_with_chromium", "angle_standalone", "angle_has_build", "angle_has_astc_encoder", "angle_enable_cl", "angle_vulkan_loader_dir"):
            blobs, args = self.fixture()
            args["targets"][0]["gnArgsWritten"][variable] = "false"
            with self.subTest(variable=variable), self.assertRaisesRegex(ValueError, "override"):
                SUBJECT["configuration"](blobs, args)
        blobs, args = self.fixture()
        args["targets"][0]["gclientConfigRecorded"]["customVars"]["build_with_chromium"] = True
        with self.assertRaisesRegex(ValueError, "override"):
            SUBJECT["configuration"](blobs, args)

    def test_missing_duplicate_or_unverified_actual_target_refused(self):
        blobs, args = self.fixture()
        for targets in (args["targets"][:-1], [args["targets"][0]] * 4, args["targets"] + [args["targets"][0]]):
            with self.subTest(targets=targets), self.assertRaisesRegex(ValueError, "all four original"):
                SUBJECT["configuration"](blobs, {"targets": targets})
        for key in ("explicitGnArgsVerified", "actualGclientCustomVarsVerified"):
            blobs, args = self.fixture()
            args["targets"][0][key] = False
            with self.assertRaisesRegex(ValueError, "independently verified"):
                SUBJECT["configuration"](blobs, args)

    def deps(self):
        return b"raise RuntimeError('must remain inert')\nvars = {'other': Var('unresolved'), 'build_with_chromium': True}\ngclient_gn_args_file = 'src/build/config/gclient_args.gni'\ngclient_gn_args = ['other', 'build_with_chromium']\n"

    def test_deps_literal_export_is_inspected_without_running_deps_or_resolving_vars(self):
        data = self.deps()
        report = SUBJECT["deps_export"](data)
        self.assertEqual(report["variableLine"], 2)
        self.assertEqual(report["sourceSha256"], hashlib.sha256(data).hexdigest())
        self.assertTrue(report["variableExported"])
        self.assertFalse(report["sourceExecuted"])
        self.assertFalse(report["remoteGeneratedFileMeasured"])

    def test_deps_nonliteral_false_duplicate_export_or_other_generated_path_refused(self):
        for old, new in ((b"'build_with_chromium': True", b"'build_with_chromium': Var('flag')"),
                         (b"'build_with_chromium': True", b"'build_with_chromium': False"),
                         (b"['other', 'build_with_chromium']", b"['build_with_chromium', 'build_with_chromium']"),
                         (b"src/build/config/gclient_args.gni", b"src/other.gni")):
            with self.subTest(new=new), self.assertRaises(ValueError):
                SUBJECT["deps_export"](self.deps().replace(old, new))
        with self.assertRaisesRegex(ValueError, "unique original"):
            SUBJECT["deps_export"](self.deps() + b"gclient_gn_args = []\n")

    def helper(self):
        return b'class Dependency:\n    def WriteGNArgsFile(self):\n        variables = self.get_vars()\n        for arg in args:\n            value = variables[arg]\n            lines.append(f"{arg} = {ToGNString(value)}")\n    def get_builtin_vars(self):\n        return {"checkout_linux": True}\n'

    def inspect_helper(self, data):
        with patch.dict(SUBJECT["GCLIENT"], load_helpers=lambda *_: ({"gclient.py": data, "gclient_eval.py": b"inert"}, "helper-seal"),
                        inspect_shapes=lambda *_: {"parentPriority": "previously checked"}):
            return SUBJECT["helper_export"](Path("fixture"))

    def test_gclient_export_writer_is_quoted_not_executed(self):
        report = self.inspect_helper(self.helper())
        self.assertEqual(report["helperSha256"], hashlib.sha256(self.helper()).hexdigest())
        self.assertEqual(report["builtinVariableKeys"], ["checkout_linux"])
        self.assertFalse(report["helperExecuted"])
        self.assertFalse(report["embeddingBuiltinOverrideDeclared"])

    def test_builtin_embedding_override_or_changed_export_writer_refused(self):
        with self.assertRaisesRegex(ValueError, "builtin embedding override"):
            self.inspect_helper(self.helper().replace(b'"checkout_linux"', b'"build_with_chromium"'))
        with self.assertRaisesRegex(ValueError, "export writer"):
            self.inspect_helper(self.helper().replace(b"value = variables[arg]", b"value = True"))

    def profile_fixture(self, source):
        root = source / SUBJECT["PROFILE_SEALS"]["targets"][0]
        root.mkdir()
        data = b"raise RuntimeError('never execute source data')\n"
        oid, digest = identity(data)
        raw = base64.b64encode(data)
        request = {"sourceGroup": "chromium", "path": "path", "sourcePath": "path", "gitObjectSha1": oid, "url": "https://fixture.test/pinned"}
        row = {**request, **digest, "rawFile": oid + ".gitiles-data", "sourceFile": oid + ".source-data",
               "rawBytes": len(raw), "rawSha256": hashlib.sha256(raw).hexdigest(), "verified": True}
        (root / row["rawFile"]).write_bytes(raw)
        (root / row["sourceFile"]).write_bytes(data)
        report = {"collectionComplete": True, "sourceCommit": SUBJECT["TREE"]["COMMIT"], "mainReceiptSha256": "main",
                  "checkoutReceiptSha256": "checkout", "sourceRoots": {key: list(value) for key, value in SUBJECT["SOURCES"]["ROOTS"].items()},
                  "checkoutCorrespondence": [], "requests": [request], "records": [row], "selectedSourceBytes": len(data)}
        for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "generatorExecuted", "effectiveGnMeasured", "shippingDecisionsMade", "metadataPathsAliased", "promoted"):
            report[key] = False
        inputs = {"mainSha256": "main", "index": {"path": {"mode": "100644", "gitObjectSha1": oid}}}
        return root, data, request, row, report, inputs

    def checked_profile(self, source, request, report, inputs):
        with patch.dict(SUBJECT["INPUTS"], receipt=lambda *_: (report, "profile-seal")), \
                patch.dict(SUBJECT["SOURCES"], requests=lambda *_: [request]):
            return SUBJECT["checked_profile"](source, "targets", inputs, "checkout", [])

    def test_raw_and_source_rechecked_independently_against_git_and_receipt(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-audit-") as name:
            source = Path(name)
            root, data, request, row, report, inputs = self.profile_fixture(source)
            blobs, seal = self.checked_profile(source, request, report, inputs)
            self.assertEqual((blobs, seal), ({"path": data}, "profile-seal"))
            (root / row["rawFile"]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "raw source bytes changed"):
                self.checked_profile(source, request, report, inputs)
            (root / row["rawFile"]).write_bytes(base64.b64encode(data))
            (root / row["sourceFile"]).write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "source bytes changed"):
                self.checked_profile(source, request, report, inputs)

    def test_original_executable_mode_is_inert_regular_data_but_links_and_other_blobs_refuse(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-mode-") as name:
            source = Path(name)
            _, data, request, _, report, inputs = self.profile_fixture(source)
            entry = inputs["index"]["path"]
            entry["mode"] = "100755"
            self.assertEqual(self.checked_profile(source, request, report, inputs)[0], {"path": data})
            for mode in ("120000", "160000", "040000"):
                entry["mode"] = mode
                with self.assertRaisesRegex(ValueError, "record identities"):
                    self.checked_profile(source, request, report, inputs)
            entry.update(mode="100755", gitObjectSha1="0" * 40)
            with self.assertRaisesRegex(ValueError, "record identities"):
                self.checked_profile(source, request, report, inputs)

    def test_profile_path_escape_identity_or_truth_claim_refused_before_data_read(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-audit-") as name:
            source = Path(name)
            _, _, request, row, report, inputs = self.profile_fixture(source)
            for key, value in (("rawFile", "../other"), ("sourceFile", "../other"), ("verified", False)):
                changed = copy.deepcopy(report)
                changed["records"][0][key] = value
                with self.subTest(key=key), self.assertRaisesRegex(ValueError, "record identities"):
                    self.checked_profile(source, request, changed, inputs)
            for key, value in (("collectionComplete", False), ("effectiveGnMeasured", True), ("metadataPathsAliased", True), ("promoted", True)):
                with self.subTest(key=key), self.assertRaisesRegex(ValueError, "fixed integration-profile receipt"):
                    self.checked_profile(source, request, {**report, key: value}, inputs)

    def test_append_keeps_real_path_existing_source_and_raw_reference(self):
        data = b"actual inert license\n"
        oid, digest = identity(data)
        inputs = {"index": {"third_party/perfetto/LICENSE": {"mode": "100644", "gitObjectSha1": oid}}, "files": {}, "references": []}
        SUBJECT["append_blobs"](inputs, {"third_party/perfetto/LICENSE": data})
        self.assertEqual(inputs["files"]["third_party/perfetto/LICENSE"]["sha256"], digest["sha256"])
        original = copy.deepcopy(inputs)
        SUBJECT["append_blobs"](inputs, {"third_party/perfetto/LICENSE": data})
        self.assertEqual(inputs, original)
        self.assertNotIn(SCREEN + "third_party/perfetto/src/LICENSE", inputs["files"])

    def test_append_foreign_blob_or_regular_source_conflict_refused(self):
        data = b"actual inert source\n"
        oid, digest = identity(data)
        for changed in ({"mode": "120000", "gitObjectSha1": oid}, {"mode": "100644", "gitObjectSha1": "a" * 40}):
            inputs = {"index": {"path": changed}, "files": {}, "references": []}
            with self.assertRaises(ValueError):
                SUBJECT["append_blobs"](inputs, {"path": data})
        inputs = {"index": {"path": {"mode": "100644", "gitObjectSha1": oid}},
                  "files": {"path": {**digest, "payload": b"other"}}, "references": []}
        with self.assertRaisesRegex(ValueError, "never replace"):
            SUBJECT["append_blobs"](inputs, {"path": data})

    def test_perfetto_tree_and_license_mutation_refused_without_nested_alias(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-audit-") as name:
            source = Path(name)
            directory = source / "perfetto-license-source-01/perfetto"
            (directory / "blobs").mkdir(parents=True)
            data = b"actual Perfetto license\n"
            oid, digest = identity(data)
            tree = ("100644 blob " + oid + "\tLICENSE\0").encode()
            (directory / "source-tree.nul").write_bytes(tree)
            blob_path = directory / "blobs" / (oid + ".source-data")
            blob_path.write_bytes(data)
            prefix, repository, revision = SUBJECT["PERFETTO"]["PINS"]["perfetto"]
            component = {"checkoutPath": prefix, "repository": repository, "revision": revision,
                         "collectionComplete": True, "generatorExecuted": False, "promoted": False,
                         "treeBytes": len(tree), "treeSha256": hashlib.sha256(tree).hexdigest(), "treeEntries": 1,
                         "rootTreeSha1": "a" * 40, "sourceBytes": len(data),
                         "selectedFiles": [{"path": "LICENSE", "mode": "100644", "gitObjectSha1": oid, **digest}]}
            report = {"collectionComplete": True, "sourceCommit": SUBJECT["TREE"]["COMMIT"], "mainReceiptSha256": "main",
                      "checkoutReceiptSha256": "checkout", "parentCatalogSha256": SUBJECT["INPUTS"]["EXTERNAL"]["CATALOG_SHA"],
                      "checkoutCorrespondence": [], "components": [{"receiptSha256": SUBJECT["PERFETTO_SHA"]}]}
            for key in ("sourceFunctionsExecuted", "conditionsEvaluated", "generatorExecuted", "shippingDecisionsMade", "metadataPathsAliased", "promoted"):
                report[key] = False
            def receipt(path, expected):
                return (report if path.name == "perfetto-source-inventory.json" else component), expected
            def inputs():
                return {"mainSha256": "main", "index": {}, "files": {}, "references": []}
            with patch.dict(SUBJECT["PERFETTO"], checked_inputs=lambda *_: ({"mainSha256": "main"}, "checkout", [])), \
                    patch.dict(SUBJECT["INPUTS"], receipt=receipt):
                merged = inputs()
                result = SUBJECT["checked_perfetto"](source, merged)
                self.assertEqual(result["selectedFiles"], 1)
                self.assertEqual(merged["files"]["third_party/perfetto/LICENSE"]["payload"], data)
                self.assertNotIn(SCREEN + "third_party/perfetto/src/LICENSE", merged["index"])
                (directory / "source-tree.nul").write_bytes(tree + b"changed")
                with self.assertRaisesRegex(ValueError, "tree bytes changed"):
                    SUBJECT["checked_perfetto"](source, inputs())
                (directory / "source-tree.nul").write_bytes(tree)
                blob_path.write_bytes(b"changed license")
                with self.assertRaisesRegex(ValueError, "fixed Git blob"):
                    SUBJECT["checked_perfetto"](source, inputs())

    def test_foreign_escaping_or_existing_output_refused_before_source_load(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-integration-audit-") as name:
            source = Path(name)
            for output in (source / "foreign.json", source / "../integration-coverage-01.json"):
                with self.assertRaises(ValueError):
                    SUBJECT["inspect"](source, output)
            output = source / "integration-coverage-01.json"
            output.write_bytes(b"original")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](source, output)
            self.assertEqual(output.read_bytes(), b"original")


if __name__ == "__main__":
    unittest.main()
