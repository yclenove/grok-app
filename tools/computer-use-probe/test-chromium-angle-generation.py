"""Generation bookkeeping must not become arbitrary execution or binary proof."""
import copy
import hashlib
import json
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-angle-generation.py")))
ANGLE = SUBJECT["ANGLE"]


class AngleGenerationTests(unittest.TestCase):
    def rules(self):
        registry = ("raise RuntimeError('must never execute')\nkhronos_xml_inputs = " + repr(["../" + path for path in SUBJECT["XML_PATHS"]])
            + "\nangle_xml_inputs = ['gl_angle_ext.xml', 'egl_angle_ext.xml', 'registry_xml.py']\n").encode()
        runner = ("generators = " + repr(SUBJECT["GENERATORS"]) + "\n" + """def md5(fname):
    hash_md5 = hashlib.md5()
    with open(fname, 'rb') as f:
        if sys.platform.startswith('win') or sys.platform == 'cygwin':
            hash_md5.update(f.read().replace(b'\\r\\n', b'\\n'))
        else:
            for chunk in iter(lambda: f.read(4096), b''):
                hash_md5.update(chunk)
    return hash_md5.hexdigest()
""").encode()
        return registry, runner

    def test_literal_input_and_generator_mapping_is_read_without_running_upstream(self):
        registry, runner = self.rules()
        result = SUBJECT["generation_rules"](registry, runner)
        self.assertEqual(result["registryXmlInputs"], list(SUBJECT["XML_PATHS"]))
        self.assertTrue(result["md5IsGenerationBookkeepingOnly"])
        self.assertFalse(result["sourceExecuted"])

    def test_changed_generator_registry_input_or_hash_normalization_refuses(self):
        registry, runner = self.rules()
        for changed_registry, changed_runner in ((registry.replace(b"api/egl.xml", b"api/other.xml"), runner),
                (registry, runner.replace(b"scripts/generate_loader.py", b"scripts/other.py")),
                (registry, runner.replace(b"f.read().replace(b'\\r\\n', b'\\n')", b"f.read()"))):
            with self.assertRaises(ValueError):
                SUBJECT["generation_rules"](changed_registry, changed_runner)

    def test_json_duplicate_escaping_absolute_and_non_md5_entries_reject(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            SUBJECT["generation_record"](b'{"a":"' + b"0" * 32 + b'","a":"' + b"1" * 32 + b'"}')
        for path in ("../file", "/file", "C:/file", "a//b", "a/./b", "a\\b", "a\nb"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                SUBJECT["generation_record"](json.dumps({path: "0" * 32}).encode())
        for value in (None, 5, True, "0" * 31, "A" * 32):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT["generation_record"](json.dumps({"src/file": value}).encode())

    def test_json_empty_nested_and_over_budget_records_reject(self):
        for data in (b"", b"{}", b"[]", b"null", b'{"src/file": {"other": 1}}', b"x" * (SUBJECT["MAX_RECORD"] + 1),
                json.dumps({"src/file" + str(number): "0" * 32 for number in range(513)}).encode()):
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                SUBJECT["generation_record"](data)

    def records(self, data=b"actual generated output\r\n"):
        path = ANGLE + "src/libEGL/egl_loader_autogen.cpp"
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        blobs, index = {path: data}, {path: {"mode": "100644", "kind": "blob", "gitObjectSha1": oid}}
        for number, name in enumerate(SUBJECT["GENERATORS"]):
            record = {key: str(i + 1) * 32 for i, key in enumerate(SUBJECT["XML_PATHS"])}
            if number == 0:
                record["src/libEGL/egl_loader_autogen.cpp"] = hashlib.md5(data.replace(b"\r\n", b"\n")).hexdigest()
            record["src/other_generated.cpp"] = "a" * 32
            filename = name.replace(" ", "_").replace("/", "_") + ".json"
            blobs[ANGLE + "scripts/code_generation_hashes/" + filename] = json.dumps(record).encode()
        return blobs, index, path

    def test_source_git_identity_and_normalized_record_match_keep_unknown_entries(self):
        blobs, index, path = self.records()
        original = copy.deepcopy((blobs, index))
        report = SUBJECT["check_records"](blobs, index)
        row = report["selectedSourceMatches"][0]
        self.assertEqual(row["sourcePath"], path)
        self.assertFalse(row["rawMd5Matches"])
        self.assertTrue(row["windowsNormalizedMd5Matches"])
        self.assertFalse(row["generatorReproduced"])
        self.assertFalse(row["binaryUseProven"])
        self.assertEqual(len(report["registryInputRecordedMd5"]), 5)
        self.assertEqual(len(report["unmeasuredRecordEntries"]), 30)
        self.assertFalse(report["allGeneratedOutputsReproduced"])
        self.assertEqual((blobs, index), original)

    def test_matching_md5_cannot_replace_original_git_blob_identity(self):
        blobs, index, path = self.records()
        index[path]["gitObjectSha1"] = "0" * 40
        with self.assertRaises(ValueError):
            SUBJECT["check_records"](blobs, index)
        blobs, index, path = self.records()
        index[path]["mode"] = "120000"
        with self.assertRaisesRegex(ValueError, "regular"):
            SUBJECT["check_records"](blobs, index)

    def test_mismatching_generation_record_refuses_even_with_valid_git_object(self):
        blobs, index, _ = self.records()
        path = ANGLE + "scripts/code_generation_hashes/GL_EGL_WGL_loader.json"
        record = json.loads(blobs[path])
        record["src/libEGL/egl_loader_autogen.cpp"] = "b" * 32
        blobs[path] = json.dumps(record).encode()
        with self.assertRaisesRegex(ValueError, "differs from original"):
            SUBJECT["check_records"](blobs, index)

    def test_registry_record_disagreement_or_missing_input_is_not_silently_accepted(self):
        blobs, index, _ = self.records()
        path = ANGLE + "scripts/code_generation_hashes/proc_table.json"
        record = json.loads(blobs[path])
        record[SUBJECT["XML_PATHS"][0]] = "f" * 32
        blobs[path] = json.dumps(record).encode()
        with self.assertRaisesRegex(ValueError, "disagree"):
            SUBJECT["check_records"](blobs, index)
        blobs, index, _ = self.records()
        for path in blobs:
            if path.endswith(".json"):
                record = json.loads(blobs[path])
                del record[SUBJECT["XML_PATHS"][0]]
                blobs[path] = json.dumps(record).encode()
        with self.assertRaisesRegex(ValueError, "all five"):
            SUBJECT["check_records"](blobs, index)

    def declarations(self):
        catalog, index, files = {"gitDependencies": []}, {}, {}
        for name, (revision, _) in SUBJECT["REGISTRIES"].items():
            relative = "third_party/" + name + "/src"
            catalog["gitDependencies"].append({"path": relative, "repository": "https://chromium.googlesource.com/external/github.com/KhronosGroup/" + name,
                "revision": revision, "condition": "not build_with_chromium", "gitlinkRevision": revision, "gitlinkMatched": True})
            index[ANGLE + relative] = {"path": relative, "mode": "160000", "kind": "commit", "gitObjectSha1": revision}
            files[ANGLE + "third_party/" + name + "/README.chromium"] = {"payload": b"Revision: DEPS\nLicense: Apache-2.0\nLicense File: Not Included\nShipped: yes\n"}
        names = ("windows-x64", "linux-x64", "mac-intel", "mac-arm64")
        contexts = [{"target": name, "effectiveFlagsFromRecordedContextAndPinnedRules": {"build_with_chromium": True}} for name in names]
        targets = [{"target": name, "manifest": {}} for name in names]
        return catalog, contexts, index, files, targets

    def test_false_recorded_condition_and_manifest_absence_never_exclude_binary(self):
        args = self.declarations()
        original = copy.deepcopy(args)
        report = SUBJECT["registry_declarations"](*args)
        self.assertEqual(len(report), 3)
        for row in report:
            self.assertFalse(row["binaryExclusionProven"])
            self.assertFalse(row["copiedHeaderProvenanceProven"])
            self.assertFalse(row["registrySourceBytesVerified"])
            self.assertEqual([item["localBooleanResult"] for item in row["recordedContextAnalysis"]], [False] * 4)
            self.assertEqual({item["status"] for item in row["originalManifestCorrespondence"]}, {"exact_manifest_entry_absent"})
            self.assertEqual(row["originalMetadataFields"]["License File"]["text"], "License File: Not Included")
        self.assertEqual(args, original)

    def test_wrong_revision_gitlink_condition_metadata_and_duplicate_target_reject(self):
        for key, value in (("revision", "0" * 40), ("gitlinkMatched", False), ("condition", "True"), ("repository", "https://example.test")):
            args = self.declarations()
            args[0]["gitDependencies"][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                SUBJECT["registry_declarations"](*args)
        args = self.declarations()
        next(iter(args[2].values()))["mode"] = "120000"
        with self.assertRaises(ValueError):
            SUBJECT["registry_declarations"](*args)
        args = self.declarations()
        next(iter(args[3].values()))["payload"] = b"Revision: DEPS\nLicense: Apache-2.0\nLicense File: LICENSE\nShipped: no\n"
        with self.assertRaisesRegex(ValueError, "metadata"):
            SUBJECT["registry_declarations"](*args)
        args = self.declarations()
        args[1][0] = args[1][1]
        with self.assertRaisesRegex(ValueError, "four unique"):
            SUBJECT["registry_declarations"](*args)

    def test_upstream_source_assignment_is_data_and_cannot_execute(self):
        data = b"raise RuntimeError('never execute upstream')\ninputs = ['src/file']\n"
        self.assertEqual(SUBJECT["assignment"](data, "inputs"), ["src/file"])
        for contents in (data + b"inputs = []\n", b"inputs = arbitrary_function()\n"):
            with self.assertRaises(ValueError):
                SUBJECT["assignment"](contents, "inputs")

    def test_output_is_fresh_and_cannot_escape_or_overwrite_original_report(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-angle-generation-inspection-") as name:
            source = Path(name)
            output = source / "angle-generation-coverage-01.json"
            output.write_bytes(b"original evidence")
            loader = Mock(side_effect=AssertionError("must not load source"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None), patch.dict(SUBJECT["SOURCES"], checked_inputs=loader):
                for path in (source / "foreign.json", source / "../angle-generation-coverage-01.json"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, path)
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, output)
            loader.assert_not_called()
            self.assertEqual(output.read_bytes(), b"original evidence")


if __name__ == "__main__":
    unittest.main()
