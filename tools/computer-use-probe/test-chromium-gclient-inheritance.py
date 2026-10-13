"""Static shapes and local examples never become actual checkout decisions."""
from pathlib import Path
import runpy
import tempfile
import unittest


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-gclient-inheritance.py")))


class InheritanceTests(unittest.TestCase):
    def payloads(self, getter=None, merge=None, parser=None, evaluator=None):
        indent = SUBJECT["textwrap"].indent
        getter = SUBJECT["GET_VARS"] if getter is None else getter
        merge = SUBJECT["MERGE_VARS"] if merge is None else merge
        parser = ("gclient_eval.Parse(deps_content, filepath, self.get_vars(), self.get_builtin_vars())\n"
                  + SUBJECT["PARENT_OVERRIDE"]) if parser is None else parser
        evaluator = SUBJECT["PARSE_VARS"] if evaluator is None else evaluator
        source = ("class Dependency:\n    def get_vars(self):\n" + indent(getter.strip(), "        ")
                  + "\n    def ParseDepsFile(self):\n" + indent(parser.strip(), "        ")
                  + "\ndef merge_vars(result, new_vars):\n" + indent(merge.strip(), "    ") + "\n")
        helper = "def Exec():\n" + indent(evaluator.strip(), "    ") + "\n"
        return source.encode(), helper.encode()

    def test_exact_source_shapes_and_line_excerpts_without_execution(self):
        source, helper = self.payloads()
        result = SUBJECT["inspect_shapes"](source, helper)
        self.assertEqual(len(result), 4)
        self.assertIn("self.custom_vars", result["getVarsPrecedence"]["sourceExcerpt"])
        self.assertGreater(result["parseOverrideForDeclaredVarsOnly"]["endLine"], 0)

    def test_parent_precedes_custom_and_builtin_in_explicit_examples(self):
        example = SUBJECT["scalar_example"](False, True)
        self.assertTrue(example["exampleEffectiveBoolean"])
        self.assertFalse(SUBJECT["scalar_example"](False, True, custom=False)["exampleEffectiveBoolean"])
        self.assertTrue(SUBJECT["scalar_example"](False, True, builtin=False, custom=True)["exampleEffectiveBoolean"])
        self.assertTrue(example["hypotheticalOnly"])
        self.assertEqual(example["actualBuilderValue"], "unknown")

    def test_strings_numbers_expressions_and_missing_defaults_are_not_evaluated(self):
        for child, parent, builtin, custom in (("False", True, None, None), (False, 1, None, None),
                                                (None, True, None, None), (False, True, "checkout_win", None),
                                                (False, True, None, 0)):
            with self.subTest(child=child, parent=parent), self.assertRaises(ValueError):
                SUBJECT["scalar_example"](child, parent, builtin, custom)

    def test_reordered_or_omitted_precedence_is_not_accepted_as_same_mechanism(self):
        original = SUBJECT["GET_VARS"]
        for getter in (original.replace("result.update(self._vars)", "result.update(self.custom_vars)"),
                       original.replace("merge_vars(result, self.custom_vars)", "pass")):
            with self.subTest(getter=getter), self.assertRaises(ValueError):
                SUBJECT["inspect_shapes"](*self.payloads(getter=getter))

    def test_wrong_parser_override_or_missing_membership_filter_reject(self):
        for parser, evaluator in (("gclient_eval.Parse(deps_content, filepath, {}, {})\n" + SUBJECT["PARENT_OVERRIDE"], None),
                                   (None, SUBJECT["PARSE_VARS"].replace("if k in vars_dict", "if True"))):
            with self.subTest(parser=parser), self.assertRaises(ValueError):
                SUBJECT["inspect_shapes"](*self.payloads(parser=parser, evaluator=evaluator))

    def test_extra_or_changed_merge_implementation_is_not_silently_modeled(self):
        for merge in (SUBJECT["MERGE_VARS"].replace("result[k] = v", "result[k] = False"),
                      SUBJECT["MERGE_VARS"] + "\nupstream_side_effect()\n"):
            with self.subTest(merge=merge), self.assertRaises(ValueError):
                SUBJECT["inspect_shapes"](*self.payloads(merge=merge))

    def test_imports_raises_and_upstream_calls_outside_selected_source_are_never_run(self):
        source, helper = self.payloads()
        SUBJECT["inspect_shapes"](b"import nonexistent_untrusted\nraise RuntimeError('not executed')\n" + source,
                                    b"untrusted_side_effect()\n" + helper)

    def test_foreign_source_and_unsafe_or_old_evidence_reject_without_upstream_execution(self):
        for name in ("../escape", "gclient-inheritance-bad.json"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                SUBJECT["inspect"](Path("foreign"), Path("foreign") / name)
        with self.assertRaises(ValueError):
            SUBJECT["inspect"](Path("foreign"), Path("foreign/gclient-inheritance-01.json"))
        with tempfile.TemporaryDirectory(prefix="grok-cu-gclient-inheritance-") as name:
            root = Path(name)
            output = root / "gclient-inheritance-01.json"
            output.write_bytes(b"retained")
            with self.assertRaises(FileExistsError):
                SUBJECT["inspect"](root, output)
            self.assertEqual(output.read_bytes(), b"retained")


if __name__ == "__main__":
    unittest.main()
