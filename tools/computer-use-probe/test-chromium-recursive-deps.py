"""Declared recursive pins and defaults never prove inherited binary inputs."""
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-recursive-deps.py")))
OID = "a" * 40


class RecursiveTests(unittest.TestCase):
    def test_dawn_pin_layout_and_standalone_defaults_are_exact_data_not_inherited_checkout(self):
        root = self.root(recursion="['src/third_party/dawn']")
        relative = self.relative("vars['unused']=False\n").replace(b"'build_with_chromium': False", b"'build_with_chromium': False, 'dawn_standalone': True")
        report = SUBJECT["build_report"](root, relative, self.rows(), "dawn")
        self.assertEqual(report["parentCheckoutPath"], "third_party/dawn")
        self.assertEqual(report["sourceCommit"], "088a8d00efd1c70b58c549d355401d115904b648")
        self.assertTrue(report["parentDeclaresComponentRecursion"])
        self.assertTrue(report["declaredDawnStandaloneDefault"])
        self.assertFalse(report["inheritanceVerified"])
        with self.assertRaises(ValueError):
            SUBJECT["build_report"](root, relative.replace(b"'dawn_standalone': True", b"'dawn_standalone': 1"), self.rows(), "dawn")

    def root(self, extra="", recursion="['src/third_party/angle']"):
        return ("vars={'build_with_chromium': True}\ndeps={}\nrecursedeps=" + recursion + "\n" + extra).encode()

    def relative(self, extra="", default="False"):
        return ("use_relative_paths=True\nvars={'build_with_chromium': " + default + "}\n"
                "deps={'third_party/foo/src': {'url': 'https://example.org/foo@" + OID + "', "
                "'condition': 'not build_with_chromium'}}\n" + extra).encode()

    def rows(self):
        return [{"path": "third_party/foo/src", "mode": "160000", "gitObjectSha1": OID}]

    def test_exact_relative_pin_and_both_defaults_are_data_not_inheritance(self):
        report = SUBJECT["build_report"](self.root(), self.relative(), self.rows())
        self.assertEqual(report["declaredBuildWithChromiumDefaults"], {"parent": True, "angleStandalone": False})
        self.assertTrue(report["parentDeclaresAngleRecursion"])
        self.assertTrue(report["gitDependencies"][0]["gitlinkMatched"])
        self.assertEqual(report["gitDependencies"][0]["condition"], "not build_with_chromium")
        for key in ("inheritanceVerified", "builderCustomVarsKnown", "conditionsEvaluated", "sourceFunctionsExecuted",
                    "recursiveSourcesFetched", "shippingDecisionsMade", "promoted"):
            self.assertFalse(report[key])

    def test_parent_absent_recursion_is_not_silently_added(self):
        report = SUBJECT["build_report"](self.root(recursion="[]"), self.relative(), self.rows())
        self.assertFalse(report["parentDeclaresAngleRecursion"])
        self.assertEqual(report["parentRecursedeps"], [])

    def test_pin_mismatch_and_uncatalogued_gitlink_remain_in_report(self):
        rows = [{"path": "third_party/foo/src", "mode": "160000", "gitObjectSha1": "b" * 40},
                {"path": "unknown/src", "mode": "160000", "gitObjectSha1": OID}]
        report = SUBJECT["build_report"](self.root(), self.relative(), rows)
        self.assertEqual(len(report["mismatches"]), 1)
        self.assertEqual(len(report["uncataloguedGitlinks"]), 2)

    def test_imports_hooks_and_dynamic_declarations_are_never_executed(self):
        report = SUBJECT["build_report"](self.root("import untrusted\nraise RuntimeError('not executed')"),
                                         self.relative("hooks=[__import__('os').system('not executed')]"), self.rows())
        self.assertFalse(report["hooksExecuted"])
        with self.assertRaises(ValueError):
            SUBJECT["build_report"](self.root(recursion="__import__('os').system('not executed')"),
                                     self.relative(), self.rows())

    def test_nonboolean_default_and_missing_or_duplicate_layout_reject(self):
        for contents in (self.relative(default="1"), self.relative(default="'False'"),
                         self.relative().replace(b"use_relative_paths=True", b"use_relative_paths=False"),
                         self.relative("use_relative_paths=True")):
            with self.subTest(contents=contents), self.assertRaises(ValueError):
                SUBJECT["build_report"](self.root(), contents, self.rows())

    def test_duplicate_traversing_or_dynamic_parent_recursion_reject(self):
        for recursion in ("['src/third_party/angle', 'src/third_party/angle']", "['src/../escape']", "['/root']",
                          "['src/']", "['src/x\\\\y']", "{'src/third_party/angle': True}"):
            with self.subTest(recursion=recursion), self.assertRaises(ValueError):
                SUBJECT["build_report"](self.root(recursion=recursion), self.relative(), self.rows())

    def test_fresh_name_and_foreign_source_reject_before_file_reads(self):
        for output in (Path("foreign/../escape"), Path("foreign/recursive-deps-angle-bad.json")):
            with self.subTest(output=output), self.assertRaises(ValueError):
                SUBJECT["inspect"](Path("foreign"), output)
        with self.assertRaises(ValueError):
            SUBJECT["inspect"](Path("foreign"), Path("foreign/recursive-deps-angle-01.json"))

    def test_existing_receipt_is_not_rewritten(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-recursive-deps-") as name:
            root = Path(name)
            output = root / "recursive-deps-angle-01.json"
            output.write_bytes(b"old evidence")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"old evidence")

    def test_openscreen_pin_recursion_and_defaults_are_distinct_from_angle(self):
        root = self.root(recursion="['src/third_party/openscreen/src']")
        report = SUBJECT["build_report"](root, self.relative(), self.rows(), "openscreen")
        pin, oid, seal = SUBJECT["component_pin"]("openscreen")
        self.assertEqual(report["sourceCommit"], pin[2])
        self.assertEqual(report["parentCheckoutPath"], "third_party/openscreen/src")
        self.assertEqual(report["declaredBuildWithChromiumDefaults"], {"parent": True, "openscreenStandalone": False})
        self.assertTrue(report["parentDeclaresComponentRecursion"])
        self.assertNotIn("parentDeclaresAngleRecursion", report)
        self.assertEqual(oid, "22bc9c6438d34f9ea6b97158bca166eb4b4279cf")
        self.assertEqual(seal, SUBJECT["INPUTS"]["BROWSER_SEALS"]["openscreen"])
        self.assertFalse(report["inheritanceVerified"])
        self.assertFalse(report["conditionsEvaluated"])

    def test_unknown_component_rejects_before_source_bytes_are_parsed_or_read(self):
        for component in ("other", "../angle", "ANGLE", "angle/inner"):
            with self.subTest(component=component), self.assertRaisesRegex(ValueError, "only compiled"):
                SUBJECT["build_report"](b"not parsed", b"not parsed", [], component)
            with self.subTest(component=component), self.assertRaisesRegex(ValueError, "only compiled"):
                SUBJECT["inspect"](Path("foreign"), Path("foreign/recursive-deps-angle-01.json"), component)

    def test_cross_component_output_name_rejects_without_source_read(self):
        with self.assertRaisesRegex(ValueError, "safe fresh recursive"):
            SUBJECT["inspect"](Path("foreign"), Path("foreign/recursive-deps-angle-01.json"), "openscreen")

    def test_missing_openscreen_recursion_is_not_guessed_from_parent_default(self):
        report = SUBJECT["build_report"](self.root(), self.relative(), self.rows(), "openscreen")
        self.assertFalse(report["parentDeclaresComponentRecursion"])
        self.assertFalse(report["shippingDecisionsMade"])


if __name__ == "__main__":
    unittest.main()
