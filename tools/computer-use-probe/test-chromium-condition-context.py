"""Condition evidence must retain uncertainty, original errors and output ownership."""
import copy
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import Mock, patch


SUBJECT = runpy.run_path(str(Path(__file__).with_name("inspect-chromium-condition-context.py")))
TARGETS = tuple(SUBJECT["INTEGRATION"]["CRASHPAD"]["ARGS"]["PLATFORMS"])


class ConditionContextTests(unittest.TestCase):
    def defaults(self):
        return {key: {"value": value} for key, value in SUBJECT["FLAGS"].items()}

    def args(self):
        return {"targets": [{"target": target, "buildId": str(number),
            "actualGclientCustomVarsVerified": True, "gclientConfigRecorded": {
                "depsFileRecorded": ".DEPS.git", "customDeps": {}, "targetOs": [], "targetCpu": [],
                "customVars": dict(SUBJECT["CUSTOM_VARS"])}} for number, target in enumerate(TARGETS)]}

    def contexts(self):
        return SUBJECT["target_contexts"](self.args(), self.defaults(), ["checkout_android"])

    def obligations(self):
        return [{"originalOmittedMetadata": {"directory": directory, "errors": ["original missing license"],
                    "shippedMetadata": "yes"}, "declarations": [{"conditionRecorded": condition}],
                "recordedManifestEntries": [{"target": target, "entries": {}} for target in TARGETS],
                "originalErrorRemoved": False, "binaryExclusionProven": False}
            for directory, condition in SUBJECT["CONDITIONS"].items()]

    def log(self, completed=True):
        command = "gclient.py " + SUBJECT["SYNC"] + " ==="
        return ("===Running " + command + "\n" + ("===Succeeded " + command + "\n" if completed else "")).encode()

    def test_three_value_boolean_logic_retains_unknowns(self):
        cases = {"False and unknown": False, "True or unknown": True, "not unknown": None,
            "True and unknown": None, "False or unknown": None, "not (False or True)": False,
            "unknown and False": False, "unknown or True": True}
        for expression, expected in cases.items():
            with self.subTest(expression=expression):
                report = SUBJECT["analyze_boolean"](expression, {})
                self.assertIs(report["localBooleanResult"], expected)
                self.assertFalse(report["actualConditionMeasured"])
                self.assertFalse(report["binaryExclusionProven"])

    def test_short_circuit_never_hides_forbidden_syntax(self):
        expressions = ("False and dangerous()", "True or value.attr", "False and value[0]",
            "a == b", "1", "'True'", "None", "[True]", "True + False", "lambda: True", "__secret")
        for expression in expressions:
            with self.subTest(expression=expression), self.assertRaises(ValueError):
                SUBJECT["analyze_boolean"](expression, {})

    def test_invalid_context_and_expression_budgets_refuse(self):
        cases = [("a", {"a": 1}), ("a", {"a": "True"}), ("a", {1: True}), ("a", {"A": True}),
            ("a", []), ("a", {"a" + str(i): True for i in range(33)}), ("", {}), (None, {}),
            (" " * SUBJECT["MAX_EXPRESSION"] + "True", {}), (" or ".join(["a"] * 40), {}), ("a and", {})]
        for expression, context in cases:
            with self.subTest(expression=expression), self.assertRaises(ValueError):
                SUBJECT["analyze_boolean"](expression, context)

    def test_known_context_and_unknown_variable_list_remain_explicit(self):
        context = {"a": False, "b": None}
        report = SUBJECT["analyze_boolean"]("a and (b or c)", context)
        self.assertEqual(report["knownContext"], context)
        self.assertEqual(report["unknownVariables"], ["b", "c"])
        self.assertIs(report["localBooleanResult"], False)
        self.assertFalse(report["upstreamConditionExecuted"])

    def test_literal_defaults_record_lines_and_source_identity(self):
        data = b"vars = {'build_with_chromium': True,\n'checkout_fuchsia_no_hooks': False}\ndeps = {}\n"
        report = SUBJECT["boolean_defaults"](data)
        self.assertEqual({key: row["value"] for key, row in report.items()}, SUBJECT["FLAGS"])
        self.assertEqual(report["build_with_chromium"]["line"], 1)
        self.assertEqual(report["checkout_fuchsia_no_hooks"]["line"], 2)
        self.assertEqual(len(report["build_with_chromium"]["sourceSha256"]), 64)

    def test_dynamic_duplicate_missing_or_changed_defaults_refuse(self):
        sources = ("{'build_with_chromium': False, 'checkout_fuchsia_no_hooks': False}",
            "{'build_with_chromium': True, 'checkout_fuchsia_no_hooks': True}",
            "{'build_with_chromium': bool(1), 'checkout_fuchsia_no_hooks': False}",
            "{'build_with_chromium': 1, 'checkout_fuchsia_no_hooks': False}",
            "{'build_with_chromium': True}",
            "{'build_with_chromium': True, 'build_with_chromium': True, 'checkout_fuchsia_no_hooks': False}")
        for source in sources:
            with self.subTest(source=source), self.assertRaises(ValueError):
                SUBJECT["boolean_defaults"](("vars = " + source + "\ndeps = {}\n").encode())
        with self.assertRaises(ValueError):
            SUBJECT["boolean_defaults"](b"vars = {}\nvars = {}\ndeps = {}\n")

    def test_builtin_dictionary_retains_names_without_executing_values(self):
        data = b"class Dependency:\n def get_builtin_vars(self):\n  return {'checkout_android': dangerous()}\n"
        self.assertEqual(SUBJECT["builtin_names"](data), ["checkout_android"])

    def test_builtin_flag_overrides_duplicate_keys_or_dynamic_shape_refuse(self):
        dictionaries = ("{'build_with_chromium': True}", "{'checkout_fuchsia_no_hooks': False}",
            "{'a': True, 'a': False}", "{123: True}", "make_vars()")
        for dictionary in dictionaries:
            with self.subTest(dictionary=dictionary), self.assertRaises(ValueError):
                SUBJECT["builtin_names"](("class Dependency:\n def get_builtin_vars(self):\n  return " + dictionary).encode())

    def test_all_four_contexts_preserve_measurement_limits_and_config(self):
        args = self.args()
        result = SUBJECT["target_contexts"](args, self.defaults(), [])
        self.assertEqual({row["target"] for row in result}, set(TARGETS))
        for source, row in zip(args["targets"], result):
            self.assertEqual(row["configuration"], source["gclientConfigRecorded"])
            self.assertEqual(row["effectiveFlagsFromRecordedContextAndPinnedRules"], SUBJECT["FLAGS"])
            for key in ("builderDepsFileBytesMeasured", "actualConditionMeasured", "binaryExclusionProven"):
                self.assertIs(row[key], False)

    def test_missing_duplicate_or_foreign_target_refuses(self):
        original = self.args()
        for targets in (original["targets"][:-1], original["targets"][:-1] + [original["targets"][0]],
                original["targets"] + [original["targets"][0]]):
            with self.assertRaises(ValueError):
                SUBJECT["target_contexts"]({"targets": targets}, self.defaults(), [])

    def test_any_config_override_or_unverified_context_refuses(self):
        changes = (("depsFileRecorded", "DEPS"), ("customDeps", {"src/angle": None}),
            ("targetOs", ["android"]), ("targetCpu", ["arm64"]), ("customVars", {}),
            ("customVars", {**SUBJECT["CUSTOM_VARS"], "build_with_chromium": False}),
            ("customVars", {**SUBJECT["CUSTOM_VARS"], "checkout_fuchsia_no_hooks": True}))
        for key, value in changes:
            args = self.args()
            args["targets"][0]["gclientConfigRecorded"][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                SUBJECT["target_contexts"](args, self.defaults(), [])
        args = self.args()
        args["targets"][0]["actualGclientCustomVarsVerified"] = False
        with self.assertRaises(ValueError):
            SUBJECT["target_contexts"](args, self.defaults(), [])

    def test_changed_defaults_or_builtin_override_refuse(self):
        for defaults in ({}, {**self.defaults(), "extra": {"value": True}},
                {**self.defaults(), "build_with_chromium": {"value": 1}}):
            with self.assertRaises(ValueError):
                SUBJECT["target_contexts"](self.args(), defaults, [])
        with self.assertRaises(ValueError):
            SUBJECT["target_contexts"](self.args(), self.defaults(), ["build_with_chromium"])

    def test_windows_prefix_never_claims_sync_completion(self):
        result = SUBJECT["sync_evidence"](self.log(completed=False), "windows-x64")
        self.assertFalse(result["syncCompletionRecorded"])
        self.assertFalse(result["fullStdoutAvailable"])
        self.assertFalse(result["postSyncHookExecutionVerified"])
        self.assertTrue(result["syncDisablesHooks"])

    def test_complete_stdout_requires_ordered_exact_success(self):
        for target in TARGETS:
            result = SUBJECT["sync_evidence"](self.log(), target)
            self.assertTrue(result["syncCompletionRecorded"])
        for target in TARGETS:
            if target != "windows-x64":
                with self.assertRaises(ValueError):
                    SUBJECT["sync_evidence"](self.log(completed=False), target)

    def test_changed_duplicate_failed_or_reversed_sync_refuses(self):
        start, success = self.log().splitlines(keepends=True)
        logs = (self.log().replace(b"--nohooks", b"--hooks"), start + start + success,
            start + success + success, success + start, start + success.replace(b"--force", b"--force --os=android"),
            start + success.replace(b"===Succeeded", b"===Failed"))
        for data in logs:
            with self.subTest(data=data), self.assertRaises(ValueError):
                SUBJECT["sync_evidence"](data, "windows-x64")
        with self.assertRaises(ValueError):
            SUBJECT["sync_evidence"](self.log(), "foreign")

    def test_all_sixteen_false_results_retain_original_errors_and_manifests(self):
        obligations = self.obligations()
        before = copy.deepcopy(obligations)
        result = SUBJECT["dependency_analysis"](obligations, self.contexts())
        self.assertEqual(obligations, before)
        self.assertEqual(len(result), 4)
        for original, row in zip(obligations, result):
            self.assertEqual(row["originalObligation"], original)
            self.assertEqual(len(row["recordedContextAnalysis"]), 4)
            self.assertTrue(all(item["localBooleanResult"] is False for item in row["recordedContextAnalysis"]))
            for key in ("originalErrorRemoved", "actualDependencyConditionMeasured",
                    "binaryExclusionProven", "completeComponentNoticeCovered"):
                self.assertIs(row[key], False)

    def test_missing_duplicate_or_changed_original_condition_refuses(self):
        original = self.obligations()
        changed = copy.deepcopy(original)
        changed[0]["declarations"][0]["conditionRecorded"] = "False"
        for rows in (original[:-1], original[:-1] + [original[0]], changed):
            with self.assertRaises(ValueError):
                SUBJECT["dependency_analysis"](rows, self.contexts())

    def test_removed_error_claim_or_manifest_entry_refuses(self):
        for key in ("originalErrorRemoved", "binaryExclusionProven"):
            rows = self.obligations()
            rows[0][key] = True
            with self.assertRaises(ValueError):
                SUBJECT["dependency_analysis"](rows, self.contexts())
        for manifests in ([{"target": TARGETS[0], "entries": {}}],
                [{"target": target, "entries": {"changed": "entry"}} for target in TARGETS]):
            rows = self.obligations()
            rows[0]["recordedManifestEntries"] = manifests
            with self.assertRaises(ValueError):
                SUBJECT["dependency_analysis"](rows, self.contexts())

    def test_unknown_or_true_condition_never_becomes_an_exclusion(self):
        for flags in ({}, {"build_with_chromium": False, "checkout_fuchsia_no_hooks": False}):
            contexts = self.contexts()
            contexts[0]["effectiveFlagsFromRecordedContextAndPinnedRules"] = flags
            with self.assertRaises(ValueError):
                SUBJECT["dependency_analysis"](self.obligations(), contexts)

    def test_unsafe_or_existing_output_refuses_before_loading_source(self):
        with tempfile.TemporaryDirectory(prefix="grok-cu-condition-output-") as name:
            source = Path(name)
            existing = source / "condition-context-01.json"
            existing.write_bytes(b"preserve original evidence")
            loader = Mock(side_effect=AssertionError("must not load source"))
            with patch.dict(SUBJECT["INPUTS"], checked_source=lambda _source: None), patch.dict(SUBJECT["DAWN"], load_dawn=loader):
                with self.assertRaises(FileExistsError):
                    SUBJECT["inspect"](source, existing)
                for bad in (source / "../condition-context-02.json", source / "condition-context-bad.json",
                        source / "arbitrary.json"):
                    with self.assertRaises(ValueError):
                        SUBJECT["inspect"](source, bad)
            loader.assert_not_called()
            self.assertEqual(existing.read_bytes(), b"preserve original evidence")


if __name__ == "__main__":
    unittest.main()
