"""Fixed policy semantics and virtual tree boundaries, not binary coverage."""
import ast
import hashlib
from pathlib import Path
import re
import runpy
import unittest
from unittest.mock import patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-license-policy.py")))


def policy():
    return {"PRUNE_PATHS": {"third_party/development"}, "PRUNE_DIRS": (".git", ".svn", "layout_tests"),
            "VCS_METADATA_DIRS": (".git", ".svn"), "ADDITIONAL_PATHS": ("v8",),
            "ADDITIONAL_PATHS_FILENAME": "additional_readme_paths.json", "SPECIAL_CASES": {},
            "METADATA_FILE_NAMES": {"README.chromium", "README.v8"},
            "MANDATORY_FIELDS": {"Name", "URL", "License", "License File", "Shipped"},
            "ALIAS_FIELDS": {"Shipped in Chromium": "Shipped"}, "OPTIONAL_FIELDS": [],
            "MULTIVALUE_FIELDS": {"License File"}, "VALUE_DELIMITER": ",",
            "PATTERN_DEPENDENCY_DIVIDER": re.compile(r"^-{20} DEPENDENCY DIVIDER -{20}$"),
            "NOT_SHIPPED": "NOT_SHIPPED", "YES": "yes", "NO": "no"}


class PolicyTests(unittest.TestCase):
    def view(self, paths, payloads=None, gitlinks=()):
        index = {path: {"mode": "100644"} for path in paths}
        index.update({path: {"mode": "160000"} for path in gitlinks})
        return SUBJECT["SourceView"](index, payloads or {})

    def meta(self, **extra):
        return {"Name": "Example", "URL": "https://example.org", "License": "MIT", **extra}

    def test_literal_policy_paths_sets_tuple_addition_without_execution(self):
        node = ast.parse("frozenset({'README.chromium'})", mode="eval").body
        self.assertEqual(SUBJECT["literal"](node, {}), frozenset({"README.chromium"}))
        node = ast.parse("os.path.join('third_party', 'component')", mode="eval").body
        self.assertEqual(SUBJECT["literal"](node, {}), "third_party/component")
        node = ast.parse("VCS + ('layout_tests',)", mode="eval").body
        self.assertEqual(SUBJECT["literal"](node, {"VCS": (".git",)}), (".git", "layout_tests"))

    def test_dynamic_import_calls_unknown_vars_keywords_and_duplicate_dict_reject(self):
        for expression in ("__import__('os').system('must not execute')", "UNKNOWN", "set(x=1)",
                           "{ 'same': 1, 'same': 2 }"):
            with self.subTest(expression=expression), self.assertRaises(ValueError):
                SUBJECT["literal"](ast.parse(expression, mode="eval").body, {})

    def test_static_call_name_without_python39_unparse_or_dynamic_resolution(self):
        self.assertEqual(SUBJECT["dotted_name"](ast.parse("os.path.join", mode="eval").body), "os.path.join")
        self.assertIsNone(SUBJECT["dotted_name"](ast.parse("factory().method", mode="eval").body))

    def test_policy_sha_checked_before_ast_and_all_fields_required(self):
        with self.assertRaisesRegex(ValueError, "SHA256"):
            SUBJECT["parse_policy"](b"invalid upstream Python must not execute")
        contents = b"import untrusted\nraise RuntimeError('not executed')\nYES='yes'\n"
        with patch.dict(SUBJECT["parse_policy"].__globals__, POLICY_SHA=hashlib.sha256(contents).hexdigest()), \
                self.assertRaisesRegex(ValueError, "all reviewed"):
            SUBJECT["parse_policy"](contents)

    def test_static_full_fields_and_update_do_not_execute_other_source_statements(self):
        values = policy()
        values["PATTERN_DEPENDENCY_DIVIDER"] = r"^-{20} DEPENDENCY DIVIDER -{20}$"
        text = "import untrusted\nraise RuntimeError('not executed')\n"
        for key, value in values.items():
            text += key + " = " + ("re.compile(" + repr(value) + ")" if key == "PATTERN_DEPENDENCY_DIVIDER" else repr(value)) + "\n"
        text += "PRUNE_PATHS.update([os.path.join('buildtools', 'third_party', 'only_build')])\n"
        contents = text.encode()
        with patch.dict(SUBJECT["parse_policy"].__globals__, POLICY_SHA=hashlib.sha256(contents).hexdigest()):
            actual = SUBJECT["parse_policy"](contents)
        self.assertIn("buildtools/third_party/only_build", actual["PRUNE_PATHS"])
        self.assertEqual(actual["YES"], "yes")

    def test_safe_relative_root_parent_refs_and_escape(self):
        self.assertEqual(SUBJECT["safe_join"]("third_party/foo", "../LICENSE"), "third_party/LICENSE")
        self.assertEqual(SUBJECT["safe_join"]("third_party/foo", "//LICENSE"), "LICENSE")
        for path in ("../../../secret", "C:/secret", "back\\slash", "x\0y"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                SUBJECT["safe_join"]("third_party/foo", path)

    def test_nested_third_party_extra_roots_and_empty_gitlinks(self):
        view = self.view(["third_party/normal/code.cc", "third_party/normal/nested/third_party/inner/code.cc",
                          "v8/LICENSE", "v8/test/third_party/not_scanned/code.cc"], gitlinks=("third_party/empty",))
        directories, _ = SUBJECT["discover"](view, policy())
        self.assertEqual(directories, ["third_party/normal", "third_party/normal/nested/third_party/inner", "v8"])

    def test_pruned_out_layout_vcs_and_development_paths_do_not_scan(self):
        paths = ["out/third_party/build/code.cc", "outside/third_party/no/code.cc",
                 "third_party/development/nested/third_party/no/code.cc", "third_party/a/layout_tests/third_party/no/code.cc",
                 ".git/third_party/no/code.cc", "third_party/a/code.cc"]
        self.assertEqual(SUBJECT["discover"](self.view(paths), policy())[0], ["third_party/a"])

    def test_additional_path_json_cycles_are_bounded_and_source_verified(self):
        paths = ["third_party/a/additional_readme_paths.json", "third_party/a/nested/README.chromium",
                 "third_party/a/nested/additional_readme_paths.json"]
        payloads = {paths[0]: b'["nested"]', paths[2]: b'[".."]'}
        actual, json_paths = SUBJECT["discover"](self.view(paths, payloads), policy())
        self.assertEqual(actual, ["third_party/a", "third_party/a/nested"])
        self.assertEqual(len(json_paths), 2)

    def test_invalid_or_escaping_additional_path_json_cannot_read_host_files(self):
        path = "third_party/a/additional_readme_paths.json"
        for data in (b'{}', b'[1]', b'["../../../outside"]'):
            with self.subTest(data=data), self.assertRaises(ValueError):
                SUBJECT["discover"](self.view([path], {path: data}), policy())

    def test_rust_version_directory_special_case(self):
        paths = ["third_party/rust/example/v1/README.chromium", "third_party/rust/chromium_crates_io/crates/example/src.cc"]
        directories, _ = SUBJECT["discover"](self.view(paths), policy())
        self.assertIn("third_party/rust/example/v1", directories)
        self.assertNotIn("third_party/rust/chromium_crates_io/crates", directories)

    def test_multiple_dependencies_and_multivalue_license_fields(self):
        data = (b"Name: one\nURL: https://example.org\nLicense: MIT\nLicense File: one.txt, /LICENSE\n"
                b"-------------------- DEPENDENCY DIVIDER --------------------\n"
                b"Name: two\nLicense File: NOT_SHIPPED\nShipped: no\n")
        result = SUBJECT["parse_metadata"](data, policy())
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0]["License File"], ["one.txt", "/LICENSE"])
        self.assertEqual(result[1]["Shipped"], "no")

    def test_duplicate_case_insensitive_field_rejects_not_last_value_wins(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            SUBJECT["parse_metadata"](b"Name: one\nname: two\n", policy())

    def test_alias_overrides_canonical_and_missing_shipped_defaults_like_upstream(self):
        view = self.view(["third_party/a/LICENSE"], {"third_party/a/LICENSE": b"text"})
        result = SUBJECT["process"](self.meta(Shipped="yes", **{"Shipped in Chromium": "no"}), "third_party/a", view, policy())
        self.assertEqual(result["shippedMetadata"], "no")
        self.assertEqual(SUBJECT["process"](self.meta(), "third_party/a", view, policy())["shippedMetadata"], "yes")
        self.assertEqual(SUBJECT["process"](self.meta(**{"License File": ["NOT_SHIPPED"]}),
                                            "third_party/a", view, policy())["shippedMetadata"], "no")

    def test_copying_fallback_and_partly_missing_declared_license_are_explicit(self):
        view = self.view(["third_party/a/COPYING"], {"third_party/a/COPYING": b"text"})
        result = SUBJECT["process"](self.meta(), "third_party/a", view, policy())
        self.assertEqual(result["copyingFallback"], "third_party/a/COPYING")
        self.assertEqual(result["errors"], [])
        self.assertEqual(result["unresolvedDeclaredReferences"][0]["path"], "third_party/a/LICENSE")
        result = SUBJECT["process"](self.meta(**{"License File": ["COPYING", "missing"]}),
                                    "third_party/a", view, policy())
        self.assertIsNone(result["copyingFallback"])
        self.assertEqual(result["availableLicensePaths"], ["third_party/a/COPYING"])
        self.assertEqual(len(result["unresolvedDeclaredReferences"]), 1)

    def test_regular_tree_file_without_verified_bytes_never_satisfies_license(self):
        result = SUBJECT["process"](self.meta(), "third_party/a", self.view(["third_party/a/LICENSE"]), policy())
        self.assertEqual(result["availableLicensePaths"], [])
        self.assertIn("shipping metadata has no collected license text", result["errors"])

    def test_unknown_shipping_case_is_not_silently_normalized_or_declared_nonshipping(self):
        view = self.view(["third_party/a/LICENSE"], {"third_party/a/LICENSE": b"text"})
        result = SUBJECT["process"](self.meta(Shipped="No"), "third_party/a", view, policy())
        self.assertEqual(result["shippedMetadata"], "No")
        self.assertNotIn(result["shippedMetadata"], (policy()["YES"], policy()["NO"]))

    def test_source_symlink_license_does_not_count_as_verified_regular_content(self):
        view = SUBJECT["SourceView"]({"third_party/a/LICENSE": {"mode": "120000"}},
                                     {"third_party/a/LICENSE": b"../../outside"})
        result = SUBJECT["process"](self.meta(), "third_party/a", view, policy())
        self.assertEqual(result["availableLicensePaths"], [])
        self.assertTrue(result["errors"])

    def test_metadata_special_cases_override_readmes_without_hiding_missing_text(self):
        current = policy()
        current["SPECIAL_CASES"] = {"third_party/a": self.meta(Name="special")}
        result = SUBJECT["scan"](self.view(["third_party/a/README.chromium"],
                                          {"third_party/a/README.chromium": b"Name: must not be read\n"}), current)
        self.assertEqual(result["entries"][0]["name"], "special")
        self.assertEqual(result["entries"][0]["metadata"], "licenses.py SPECIAL_CASES")
        self.assertTrue(result["entries"][0]["errors"])

    def test_missing_metadata_duplicate_fields_and_missing_license_stay_errors(self):
        paths = ["third_party/a/code.cc", "third_party/b/README.chromium", "third_party/c/README.chromium"]
        payloads = {paths[1]: b"Name: one\nName: two\n", paths[2]: b"Name: c\nURL: https://c\nLicense: MIT\n"}
        result = SUBJECT["scan"](self.view(paths, payloads), policy())
        self.assertEqual(len(result["directoryErrors"]), 2)
        self.assertEqual(len(result["entries"]), 1)
        self.assertTrue(result["entries"][0]["errors"])


if __name__ == "__main__":
    unittest.main()
