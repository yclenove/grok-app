import ast
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("deps_pins", Path(__file__).with_name("inspect-chromium-deps-pins.py"))
SUBJECT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUBJECT)
OID = "a" * 40


class DepsTests(unittest.TestCase):
    def test_nongit_directory_destination_preserves_slash_without_fabricating_git_identity(self):
        contents = b"vars={}\ndeps={'third_party/direct3d-warp/bin/': {'dep_type': 'cipd', 'condition': 'checkout_win', 'packages': [{'package':'name','version':'pin'}]}}"
        report = SUBJECT.inspect(contents, [], path_prefix="", source_commit=OID)
        self.assertEqual(report["nonGitDependencies"][0]["path"], "third_party/direct3d-warp/bin/")
        self.assertTrue(report["nonGitDependencies"][0]["directoryPathDeclared"])
        self.assertEqual(report["nonGitDependencies"][0]["condition"], "checkout_win")
        self.assertFalse(report["conditionsEvaluated"])
        self.assertEqual(report["gitDependencies"], [])
        self.assertEqual(report["unresolved"], [])

    def test_directory_suffix_cannot_allow_git_traversal_double_slashes_or_executable_kind(self):
        for path, kind in (("third_party/repo/", "'git'"), ("third_party/x//", "'cipd'"),
                           ("../escape/", "'gcs'"), ("/root/", "'cipd'"), ("x/../escape/", "'cipd'"),
                           ("x/", "__import__('os').system('not executed')")):
            contents = ("vars={}\ndeps={" + repr(path) + ": {'dep_type': " + kind + "}}").encode()
            with self.subTest(path=path), self.assertRaises(ValueError):
                SUBJECT.inspect(contents, [], path_prefix="", source_commit=OID)

    def node(self, text):
        return ast.parse(text, mode="eval").body

    def test_string_constants_additions_var_and_str_only(self):
        variables = {"host": self.node("'https://example.org'"), "pin": self.node(repr(OID))}
        value = SUBJECT.literal(self.node("Var('host') + '/repo.git@' + Var('pin')"), variables)
        self.assertEqual(value, "https://example.org/repo.git@" + OID)
        self.assertEqual(SUBJECT.literal(self.node("Str('version')"), {}), "version")

    def test_calls_attributes_comprehensions_and_math_are_never_executed(self):
        for text in ("__import__('os').system('unsafe')", "exec('unsafe')", "obj.field",
                     "[f() for x in y]", "1 + 2", "Str(1)", "Var('missing')"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                SUBJECT.literal(self.node(text), {})

    def test_cycle_and_duplicate_dictionary_reject(self):
        with self.assertRaises(ValueError):
            SUBJECT.literal(self.node("Var('cycle')"), {"cycle": self.node("Var('cycle')")})
        with self.assertRaises(ValueError):
            SUBJECT.dictionary(self.node("{'key': 1, 'key': 2}"))

    def test_pin_requires_exact_sha_https_without_credentials_or_query(self):
        for value in ("https://example.org/repo.git@main", "http://example.org/repo.git@" + OID,
                      "https://user:secret@example.org/repo.git@" + OID,
                      "https://example.org/repo.git?key=secret@" + OID):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT.git_pin(value)
        self.assertEqual(SUBJECT.git_pin("https://example.org/repo.git@" + OID),
                         ("https://example.org/repo.git", OID))

    def test_canonical_git_repository_url_does_not_require_dot_git_suffix(self):
        self.assertEqual(SUBJECT.git_pin("https://chromium.googlesource.com/devtools/devtools-frontend@" + OID),
                         ("https://chromium.googlesource.com/devtools/devtools-frontend", OID))
        for url in ("https://example.org/@", "https://example.org/a b@", "https://example.org/a\\b@"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                SUBJECT.git_pin(url + OID)

    def test_simple_url_placeholders_are_literal_not_python_format_expressions(self):
        variables = {"host": self.node("'https://example.org'")}
        self.assertEqual(SUBJECT.expand_url_variables("{host}/repo@" + OID, variables),
                         "https://example.org/repo@" + OID)
        for value in ("{host.__class__}/repo", "{host[0]}/repo", "{host!r}/repo", "{unknown}/repo"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                SUBJECT.expand_url_variables(value, variables)

    def test_gitlink_match_and_unevaluated_condition(self):
        contents = ("vars = {'pin': '" + OID + "'}\ndeps = {'src/v8': {'url': "
                    "'https://example.org/v8.git@' + Var('pin'), 'condition': 'checkout_mac'}}").encode()
        report = SUBJECT.inspect(contents, [{"path": "v8", "mode": "160000", "gitObjectSha1": OID}])
        self.assertEqual(report["gitDependencies"][0]["condition"], "checkout_mac")
        self.assertTrue(report["gitDependencies"][0]["gitlinkMatched"])
        self.assertFalse(report["conditionsEvaluated"])
        self.assertEqual(report["uncataloguedGitlinks"], [])

    def test_mismatch_and_missing_are_evidence_not_silently_removed(self):
        contents = ("vars = {}\ndeps = {'src/v8': 'https://example.org/v8.git@" + OID + "'}").encode()
        rows = [{"path": "v8", "mode": "160000", "gitObjectSha1": "b" * 40},
                {"path": "other", "mode": "160000", "gitObjectSha1": OID}]
        report = SUBJECT.inspect(contents, rows)
        self.assertEqual(len(report["mismatches"]), 1)
        self.assertEqual(len(report["uncataloguedGitlinks"]), 2)

    def test_cipd_gcs_and_executable_hook_remain_data_only(self):
        contents = b"""vars = {}
deps = {'src/tool': {'dep_type': 'cipd', 'packages': [{'package': 'name', 'version': 'pin'}]},
        'src/asset': {'dep_type': 'gcs', 'objects': [{'sha256sum': 'hash', 'size_bytes': 3}]},
        'src/unsafe': __import__('os').system('must not execute')}
hooks = [__import__('os').system('must not execute')]
"""
        report = SUBJECT.inspect(contents, [])
        self.assertEqual(len(report["nonGitDependencies"]), 2)
        self.assertEqual(len(report["unresolved"]), 1)
        self.assertFalse(report["hooksExecuted"])
        self.assertFalse(report["dependencySourcesCollected"])
        self.assertEqual(report["licenseCoverage"], "not_proven")

    def test_repository_relative_layout_is_explicit_and_has_its_own_commit(self):
        contents = ("vars={}\ndeps={'third_party/lib/src': {'url': 'https://example.org/lib@" + OID
                    + "', 'condition': 'not build_with_chromium'}}").encode()
        with self.assertRaises(ValueError):
            SUBJECT.inspect(contents, [])
        report = SUBJECT.inspect(contents, [{"path": "third_party/lib/src", "mode": "160000", "gitObjectSha1": OID}],
                                 path_prefix="", source_commit="b" * 40)
        self.assertEqual(report["sourceCommit"], "b" * 40)
        self.assertEqual(report["gitDependencies"][0]["condition"], "not build_with_chromium")
        self.assertTrue(report["gitDependencies"][0]["gitlinkMatched"])
        self.assertFalse(report["conditionsEvaluated"])

    def test_layout_traversal_absolute_empty_control_and_windows_paths_reject(self):
        for path in ("", "/root", "../escape", "x/../escape", "x//y", "C:/x", "x\\y", "x\0y"):
            contents = ("vars={}\ndeps={" + repr(path) + ": 'https://example.org/lib@" + OID + "'}").encode()
            with self.subTest(path=path), self.assertRaises(ValueError):
                SUBJECT.inspect(contents, [], path_prefix="", source_commit=OID)
        for prefix, commit in (("../", OID), ("src/", "main")):
            with self.subTest(prefix=prefix, commit=commit), self.assertRaises(ValueError):
                SUBJECT.inspect(b"not parsed", [], path_prefix=prefix, source_commit=commit)


if __name__ == "__main__":
    unittest.main()
