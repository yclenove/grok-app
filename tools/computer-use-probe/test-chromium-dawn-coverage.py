"""Dawn override/notice corruption refuses; no network, upstream code or GN."""
import hashlib
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-dawn-coverage.py")))


class DawnTests(unittest.TestCase):
    def blobs(self):
        paths = {**{key: row[0] for key, row in SUBJECT["LICENSE_PATHS"].items()}, **SUBJECT["UNBOUND_PATHS"]}
        text = "\n".join(key + ' = "' + value + '"' for key, value in paths.items()) + "\n"
        helper = b'import("//build_overrides/dawn.gni")\nif (!defined(dawn_standalone)) {\n  dawn_standalone = false\n}\n'
        return {"build_overrides/dawn.gni": text.encode(), SUBJECT["ROOT"] + "scripts/dawn_overrides_with_defaults.gni": helper}

    def inputs(self):
        paths = {row[1] for row in SUBJECT["LICENSE_PATHS"].values()} | {SUBJECT["ROOT"] + "LICENSE"}
        data = b"original copyright\ncomplete permission\ncomplete disclaimer\n"
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        return {"index": {path: {"mode": "100644", "gitObjectSha1": oid} for path in paths},
            "files": {path: {"payload": data, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()} for path in paths}}

    def test_original_overrides_include_all_seventeen_directories(self):
        report = SUBJECT["overrides"](self.blobs())
        self.assertEqual(len(report["declaredDirectories"]), 17)
        self.assertEqual(report["declaredDirectories"]["dawn_vulkan_validation_layers_dir"], "//third_party/vulkan-validation-layers/src")
        self.assertFalse(report["sourceFunctionsExecuted"])
        self.assertFalse(report["effectiveGnMeasured"])

    def test_duplicate_redirected_missing_or_dynamic_overrides_refuse(self):
        original = self.blobs()
        data = original["build_overrides/dawn.gni"]
        for changed in (data + data.splitlines(keepends=True)[0],
                data.replace(b'"//third_party/angle"', b'"//other/angle"'), data.split(b"\n", 1)[1],
                data.replace(b'"//third_party/angle"', b'getenv("untrusted")')):
            with self.assertRaises(ValueError):
                SUBJECT["overrides"]({**original, "build_overrides/dawn.gni": changed})

    def test_standalone_default_or_override_import_change_refuses(self):
        blobs = self.blobs()
        path = SUBJECT["ROOT"] + "scripts/dawn_overrides_with_defaults.gni"
        for data in (blobs[path].replace(b"false", b"true"), blobs[path].replace(b"build_overrides/dawn", b"other/dawn")):
            with self.assertRaises(ValueError):
                SUBJECT["overrides"]({**blobs, path: data})

    def test_consumer_binding_preserves_actual_library_path_and_never_proves_binary_use(self):
        blobs, inputs = self.blobs(), self.inputs()
        blobs[SUBJECT["ROOT"] + "src/dawn/native/BUILD.gn"] = b'deps = [ "${dawn_vulkan_loader_dir}:libvulkan" ]\n'
        rows = SUBJECT["configured_bindings"](inputs, blobs, SUBJECT["overrides"](blobs))
        row = next(row for row in rows if row["variable"] == "dawn_vulkan_loader_dir")
        self.assertEqual(row["license"]["sourcePath"], "third_party/vulkan-loader/src/LICENSE.txt")
        self.assertEqual(row["consumerReferences"][0]["line"], 1)
        self.assertFalse(row["completeComponentNoticeCovered"])
        self.assertFalse(row["binaryUseProven"])
        inputs["files"][row["license"]["sourcePath"]]["payload"] = b"different source"
        with self.assertRaises(ValueError):
            SUBJECT["configured_bindings"](inputs, blobs, SUBJECT["overrides"](blobs))

    def test_fragment_retains_full_source_copyright_permission_and_disclaimer(self):
        inputs, blobs = self.inputs(), self.blobs()
        bindings = SUBJECT["configured_bindings"](inputs, blobs, SUBJECT["overrides"](blobs))
        root = SUBJECT["SUPPLEMENT"]["source_license"](inputs, SUBJECT["ROOT"] + "LICENSE")
        apache = b"full Apache standard text\n"
        pin = {"oid": hashlib.sha1(b"blob " + str(len(apache)).encode() + b"\0" + apache).hexdigest(),
               "bytes": len(apache), "sha256": hashlib.sha256(apache).hexdigest()}
        with patch.dict(SUBJECT["SUPPLEMENT"]["SPDX"]["LICENSES"], {"Apache-2.0": pin}):
            result = SUBJECT["render_fragment"](bindings, root, inputs, apache)
            self.assertIn(b"original copyright\ncomplete permission\ncomplete disclaimer\n", result)
            self.assertIn(apache, result)
            with self.assertRaises(ValueError):
                SUBJECT["render_fragment"](bindings, root, inputs, apache + b"changed")

    def test_fragment_refuses_changed_binding_even_when_payload_itself_is_valid(self):
        inputs, blobs = self.inputs(), self.blobs()
        bindings = SUBJECT["configured_bindings"](inputs, blobs, SUBJECT["overrides"](blobs))
        root = SUBJECT["SUPPLEMENT"]["source_license"](inputs, SUBJECT["ROOT"] + "LICENSE")
        bindings[0]["license"]["sha256"] = "0" * 64
        with self.assertRaises(ValueError):
            SUBJECT["render_fragment"](bindings, root, inputs, b"unused")

    def test_unresolved_library_dirs_stay_explicit_and_do_not_borrow_other_roots(self):
        self.assertEqual(SUBJECT["UNBOUND_PATHS"], {"dawn_googletest_dir": "//third_party/googletest/src",
            "dawn_partition_alloc_dir": "//base/allocator/partition_allocator"})
        self.assertEqual(set(SUBJECT["UNBOUND_PATHS"]) & set(SUBJECT["LICENSE_PATHS"]), set())

    def test_unsafe_or_existing_output_cannot_overwrite_or_start_inspection(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-dawn-coverage-") as name:
            source = Path(name)
            output = source / "dawn-coverage-01"
            output.mkdir()
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, output)
                for bad in (source / "../dawn-coverage-02", source / "dawn-coverage-bad"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, bad)


if __name__ == "__main__":
    unittest.main()
